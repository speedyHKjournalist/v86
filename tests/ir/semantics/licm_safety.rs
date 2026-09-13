use super::*;
use crate::ir::{
    backend::wasm::{emit, StateLayout},
    frontend::{decode::GuestEip, integer::IntegerBuilder},
    lowering::lower,
    state::{FlagState, ResumeKind, StateMap},
    types::Type,
};

fn snapshot(r: &mut Region, gpr: [ValueId; 8], flags: &FlagState, pc: u32) -> StateId {
    r.state(StateMap {
        instruction_pc: GuestEip(pc),
        next_pc: GuestEip(pc + 1),
        next_value: None,
        resume: ResumeKind::BeforeInstruction,
        gpr,
        flags: flags.clone(),
        xmm: vec![],
        x87: vec![],
        committed_instructions: 0,
        count_base: None,
        rep_progress: None,
    })
}

struct Fixture {
    region: Region,
    entry: BlockId,
    body: BlockId,
    invariant: InstId,
    dependent: InstId,
    read: InstId,
    polls: [InstId; 2],
}
fn instruction(r: &Region, value: ValueId) -> InstId {
    match r.values[value.index()].definition {
        Definition::Instruction(id, _) => id,
        _ => panic!("expected instruction"),
    }
}
fn fixture() -> Fixture {
    let mut b = IntegerBuilder::new();
    let entry = b.block;
    let input = b.gpr;
    let flags = b.flags.clone();
    // IDs deliberately disagree with dominance order.
    let body = b.region.block(false);
    let header = b.region.block(false);
    let exit = b.region.block(false);
    let effect = b.region.param(header, Type::Effect);
    let count = b.region.param(header, Type::I32);
    let sum = b.region.param(header, Type::I32);
    let body_effect = b.region.param(body, Type::Effect);
    let zero = b.constant(0, Type::I32);
    b.region.terminate(entry, Terminator::Branch(Edge {
        target: header, args: vec![b.effect, input[2], zero],
    }));
    let mut current = input;
    current[2] = count;
    current[3] = sum;
    for block in [header, body, exit] {
        let s = snapshot(&mut b.region, current, &flags, 0x2000 + block.0);
        b.region.blocks[block.index()].entry_state = Some(s);
    }
    b.block = header;
    let done = b.binary(Binary::Eq, count, zero);
    b.region.terminate(header, Terminator::CondBranch {
        condition: done,
        taken: Edge { target: exit, args: vec![] },
        not_taken: Edge { target: body, args: vec![effect] },
    });
    b.block = body;
    let before = b.region.blocks[body.index()].entry_state;
    let poll0 = InstId(b.region.instructions.len() as u32);
    let effect = b.region.append(body, Op::PollBudget, vec![body_effect], &[Type::Effect], before)[0];
    let add = b.binary(Binary::Add, input[0], input[1]);
    let invariant = instruction(&b.region, add);
    let square = b.binary(Binary::Mul, add, add);
    let next_sum = b.binary(Binary::Add, sum, square);
    let dependent = instruction(&b.region, next_sum);
    let read_value = b.node(Op::ReadGpr(4), vec![], Type::I32);
    let read = instruction(&b.region, read_value);
    // The recovery-only use must remain live after motion.
    current[7] = add;
    let after = snapshot(&mut b.region, current, &flags, 0x3000);
    let poll1 = InstId(b.region.instructions.len() as u32);
    let effect = b.region.append(body, Op::PollBudget, vec![effect], &[Type::Effect], Some(after))[0];
    let one = b.constant(1, Type::I32);
    let next_count = b.binary(Binary::Sub, count, one);
    b.region.terminate(body, Terminator::Branch(Edge {
        target: header, args: vec![effect, next_count, next_sum],
    }));
    let state = snapshot(&mut b.region, {
        let mut state = input;
        state[2] = count;
        state[3] = sum;
        state
    }, &flags, 0x4000);
    b.region.terminate(exit, Terminator::Exit(state));
    Fixture { region: b.region, entry, body, invariant, dependent, read, polls: [poll0, poll1] }
}

#[test]
fn hoists_dependency_chain_and_preserves_recovery_effects_and_edges() {
    let mut f = fixture();
    verify(&f.region).unwrap();
    let blocks: Vec<_> = f.region.blocks.iter().map(|b| {
        format!("{:?} {:?} {:?}", b.params, b.terminator, b.entry_state)
    }).collect();
    let states = format!("{:?}", f.region.states);
    let stats = run(&mut f.region, DEFAULT_WORK_LIMIT).unwrap();
    assert_eq!(stats.loops, 1);
    assert_eq!(stats.hoisted, 3);
    assert_eq!(f.region.instructions[f.invariant.index()].block, f.entry);
    for id in [f.dependent, f.read, f.polls[0], f.polls[1]] {
        assert_eq!(f.region.instructions[id.index()].block, f.body);
    }
    assert_eq!(format!("{:?}", f.region.states), states);
    for (b, expected) in f.region.blocks.iter().zip(blocks) {
        assert_eq!(format!("{:?} {:?} {:?}", b.params, b.terminator, b.entry_state), expected);
    }
    verify(&f.region).unwrap();
    lower(&f.region).unwrap();
    assert_eq!(run(&mut f.region, DEFAULT_WORK_LIMIT).unwrap().hoisted, 0);
}

#[test]
fn state_bearing_expression_is_not_speculated() {
    let mut f = fixture();
    f.region.instructions[f.invariant.index()].state = f.region.blocks[f.body.index()].entry_state;
    let stats = run(&mut f.region, DEFAULT_WORK_LIMIT).unwrap();
    assert_eq!(stats.hoisted, 1);
    assert_eq!(f.region.instructions[f.invariant.index()].block, f.body);
}

#[test]
fn unused_arena_definitions_are_not_dereferenced() {
    let mut f = fixture();
    f.region.values.push(Value {
        ty: Type::I32, definition: Definition::Instruction(InstId(u32::MAX), 0),
    });
    verify(&f.region).unwrap();
    assert_eq!(run(&mut f.region, DEFAULT_WORK_LIMIT).unwrap().hoisted, 3);
}

#[test]
fn emit_licm_reference_and_optimized_modules() {
    let layout = StateLayout { gpr: 0, flags: 32, eip: 36, committed: 40, flag_operand: 44 };
    std::fs::create_dir_all("build/ir-licm-safety").unwrap();
    for budget in [1, 2, 3, 4, 5, 8, 16, 64, 256] {
        for optimized in [false, true] {
            let mut f = fixture();
            if optimized {
                assert_eq!(run(&mut f.region, DEFAULT_WORK_LIMIT).unwrap().hoisted, 3);
            }
            let mir = lower(&f.region).unwrap();
            drop(f.region);
            std::fs::write(format!("build/ir-licm-safety/loop-{budget}-{optimized}.wasm"),
                emit(&mir, layout, budget).unwrap().bytes).unwrap();
        }
    }
}

#[test]
fn compile_request_only_runs_licm_in_optimized_tier_two() {
    use crate::ir::{
        frontend::decode::{LinearAddress, PhysicalAddress},
        passes::PassConfig,
        runtime::compile::{
            compile_cpu_cfg_region, CodeDependency, CodeMapping, CompileRequest,
            ImmutableCodeSnapshot, IrConfig, PublicationKey, Tier,
        },
    };
    let mut request = CompileRequest {
        key: PublicationKey { job: 1, vm_generation: 1, slot: 0, slot_generation: 1 },
        pc: GuestEip(0x1000), linear: LinearAddress(0x100000), default_32: true,
        tier: Tier::One,
    };
    let snapshot = ImmutableCodeSnapshot {
        // mov ecx,4; loop: mov eax,esi; add eax,edi; dec ecx; jnz loop
        bytes: vec![0xB9, 4, 0, 0, 0, 0x8B, 0xC6, 0x03, 0xC7, 0x49, 0x75, 0xF9],
        dependencies: vec![CodeDependency { page: PhysicalAddress(0x200000), version: 1 }],
        mappings: vec![CodeMapping { linear: LinearAddress(0x100000), physical: PhysicalAddress(0x200000) }],
    };
    let mut config = IrConfig {
        optimize: true, passes: PassConfig::default(), execution_budget: 128,
        rep_iteration_budget: 8, max_code_bytes: 1920,
        layout: StateLayout { gpr: 0, flags: 32, eip: 36, committed: 40, flag_operand: 44 },
    };
    let tier_one = compile_cpu_cfg_region(&request, &snapshot, &config).unwrap();
    assert_eq!(tier_one.passes.loop_hoisted, 0);
    request.tier = Tier::Two;
    let tier_two = compile_cpu_cfg_region(&request, &snapshot, &config).unwrap();
    assert!(tier_two.passes.loop_hoisted > 0);
    config.optimize = false;
    assert_eq!(compile_cpu_cfg_region(&request, &snapshot, &config).unwrap().passes.loop_hoisted, 0);
    config.optimize = true;
    config.passes.rounds = 0;
    assert_eq!(compile_cpu_cfg_region(&request, &snapshot, &config).unwrap().passes.loop_hoisted, 0);
}
