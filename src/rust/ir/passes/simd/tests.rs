use crate::ir::{
    backend::wasm::emit_cpu,
    frontend::{decode::GuestEip, integer::IntegerBuilder},
    hir::*,
    lowering::lower,
    passes::{run, PassConfig},
    simd::PackedOp,
    state::{ResumeKind, StateMap},
    types::Type,
    verify::verify,
};

#[derive(Clone)]
enum Node {
    Shuffle(usize, usize, [u8; 16]),
    And(usize, usize),
    Or(usize, usize),
}
struct Case {
    nodes: Vec<Node>,
    output: usize,
}
fn config() -> PassConfig {
    PassConfig {
        prune: false,
        merge: false,
        phis: false,
        fold: false,
        gvn: false,
        licm: false,
        simd: true,
        dce: true,
        rounds: 1,
    }
}
fn pattern(seed: u32) -> [u8; 16] {
    let mut x = seed.wrapping_add(1);
    std::array::from_fn(|_| {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        (x & 31) as u8
    })
}
fn cases() -> Vec<Case> {
    let identity = std::array::from_fn(|i| i as u8);
    let right = std::array::from_fn(|i| 16 + i as u8);
    let duplicate = std::array::from_fn(|i| i as u8 + if i % 2 == 0 { 0 } else { 16 });
    let all_four = std::array::from_fn(|i| i as u8 + if i % 4 < 2 { 0 } else { 16 });
    let two_of_four = std::array::from_fn(|i| (i % 8 * 2) as u8 + if i % 2 == 0 { 0 } else { 16 });
    let mut cases = vec![
        Case {
            nodes: vec![Node::Shuffle(0, 1, identity)],
            output: 8,
        },
        Case {
            nodes: vec![Node::Shuffle(0, 1, right)],
            output: 8,
        },
        Case {
            nodes: vec![Node::Shuffle(0, 0, duplicate)],
            output: 8,
        },
        Case {
            nodes: vec![Node::And(0, 0)],
            output: 8,
        },
        Case {
            nodes: vec![Node::Or(0, 0)],
            output: 8,
        },
        Case {
            nodes: vec![
                Node::Shuffle(0, 1, duplicate),
                Node::Shuffle(2, 3, duplicate),
                Node::Shuffle(8, 9, all_four),
            ],
            output: 10,
        },
        Case {
            nodes: vec![
                Node::Shuffle(0, 1, duplicate),
                Node::Shuffle(2, 3, duplicate),
                Node::Shuffle(8, 9, two_of_four),
            ],
            output: 10,
        },
    ];
    for seed in 0..64 {
        cases.push(Case {
            nodes: vec![
                Node::Shuffle(0, 1, pattern(seed)),
                Node::Shuffle(1, 0, pattern(seed + 91)),
                Node::Shuffle(8, 9, pattern(seed + 177)),
                Node::Shuffle(10, 10, pattern(seed + 333)),
            ],
            output: 11,
        });
    }
    cases
}
fn region(case: &Case) -> Region {
    let mut b = IntegerBuilder::new();
    let mut values: Vec<_> = (0..8)
        .map(|i| b.node(Op::ReadXmm(i), vec![], Type::V128))
        .collect();
    let mut xmm = values.clone();
    for node in &case.nodes {
        let (op, a, c) = match node {
            Node::Shuffle(a, c, lanes) => (Op::VectorShuffle(*lanes), *a, *c),
            Node::And(a, c) => (Op::VectorBinary(PackedOp::And), *a, *c),
            Node::Or(a, c) => (Op::VectorBinary(PackedOp::Or), *a, *c),
        };
        let value = b.node(op, vec![values[a], values[c]], Type::V128);
        values.push(value);
    }
    let result = values[case.output];
    xmm[4] = result;
    for lane in 0..4 {
        b.gpr[lane] = b.node(
            Op::VectorExtract {
                bits: 32,
                lane: lane as u8,
            },
            vec![result],
            Type::I32,
        );
    }
    let state = b.region.state(StateMap {
        instruction_pc: GuestEip(0x1000),
        next_pc: GuestEip(0x1002),
        next_value: None,
        resume: ResumeKind::AfterInstruction,
        gpr: b.gpr,
        flags: b.flags,
        xmm,
        x87: vec![],
        committed_instructions: 1,
        count_base: None,
        rep_progress: None,
    });
    b.region.terminate(b.block, Terminator::Exit(state));
    b.region
}
fn shuffle_count(r: &Region) -> usize {
    r.blocks
        .iter()
        .flat_map(|b| &b.instructions)
        .filter(|i| matches!(r.instructions[i.index()].op, Op::VectorShuffle(_)))
        .count()
}

#[test]
fn identities_update_recovery_values_and_obey_opt_out() {
    for case in cases().iter().take(5) {
        let mut r = region(case);
        let original = r.clone();
        let stats = run(&mut r, config()).unwrap();
        assert_eq!(stats.simd_simplified, 1);
        let result = r.states.last().unwrap().xmm[4];
        let Definition::Instruction(id, _) = r.values[result.index()].definition else {
            panic!()
        };
        assert!(matches!(r.instructions[id.index()].op, Op::ReadXmm(_)));
        verify(&r).unwrap();
        lower(&r).unwrap();
        let mut disabled = original;
        assert_eq!(
            run(
                &mut disabled,
                PassConfig {
                    simd: false,
                    ..config()
                }
            )
            .unwrap()
            .simd_simplified,
            0
        );
    }
}
#[test]
fn four_independent_sources_are_not_silently_dropped() {
    let cases = cases();
    let mut four = region(&cases[5]);
    run(&mut four, config()).unwrap();
    assert_eq!(shuffle_count(&four), 3);
    let mut two = region(&cases[6]);
    assert!(run(&mut two, config()).unwrap().simd_simplified > 0);
    assert_eq!(shuffle_count(&two), 1);
}
#[test]
fn composition_is_bounded_and_reaches_a_fixed_point() {
    for case in cases().iter().skip(7) {
        let mut r = region(case);
        let stats = run(&mut r, config()).unwrap();
        assert!(stats.simd_simplified > 0);
        assert!(shuffle_count(&r) <= 1);
        assert_eq!(run(&mut r, config()).unwrap().simd_simplified, 0);
    }
}
#[test]
fn simd_fixtures_and_independent_byte_model() {
    std::fs::create_dir_all("build/ir-simd-opt").unwrap();
    let mut descriptions = Vec::new();
    for (index, case) in cases().iter().enumerate() {
        let nodes: Vec<_> = case
            .nodes
            .iter()
            .map(|node| match node {
                Node::Shuffle(a, b, lanes) => format!("[\"shuffle\",{a},{b},{lanes:?}]"),
                Node::And(a, b) => format!("[\"and\",{a},{b}]"),
                Node::Or(a, b) => format!("[\"or\",{a},{b}]"),
            })
            .collect();
        descriptions.push(format!(
            "{{\"nodes\":[{}],\"output\":{}}}",
            nodes.join(","),
            case.output
        ));
        for optimized in [false, true] {
            let mut r = region(case);
            if optimized {
                run(&mut r, config()).unwrap();
            }
            std::fs::write(
                format!("build/ir-simd-opt/{index}-{optimized}.wasm"),
                emit_cpu(&lower(&r).unwrap(), 100).unwrap().bytes,
            )
            .unwrap();
        }
    }
    std::fs::write(
        "build/ir-simd-opt/cases.json",
        format!("[{}]", descriptions.join(",")),
    )
    .unwrap();
}
