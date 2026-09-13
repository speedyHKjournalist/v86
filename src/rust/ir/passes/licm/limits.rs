use super::*;
use crate::ir::{
    frontend::{decode::GuestEip, integer::IntegerBuilder},
    state::{ResumeKind, StateMap},
    types::Type,
};

fn fixture(looping: bool) -> Region {
    let mut b = IntegerBuilder::new();
    if looping {
        let header = b.region.block(false);
        let effect = b.region.param(header, Type::Effect);
        let count = b.region.param(header, Type::I32);
        b.region.terminate(
            b.block,
            Terminator::Branch(Edge { target: header, args: vec![b.effect, b.gpr[0]] }),
        );
        let next = b.region.append(
            header,
            Op::Binary(Binary::Add),
            vec![count, count],
            &[Type::I32],
            None,
        )[0];
        b.region.terminate(
            header,
            Terminator::Branch(Edge { target: header, args: vec![effect, next] }),
        );
    } else {
        let state = b.region.state(StateMap {
            instruction_pc: GuestEip(0x1000),
            next_pc: GuestEip(0x1001),
            next_value: None,
            resume: ResumeKind::BeforeInstruction,
            gpr: b.gpr,
            flags: b.flags.clone(),
            xmm: vec![],
            x87: vec![],
            committed_instructions: 0,
            count_base: None,
            rep_progress: None,
        });
        b.region.terminate(b.block, Terminator::Exit(state));
    }
    b.region
}

#[test]
fn unchanged_regions_retain_their_original_arenas() {
    for looping in [false, true] {
        let mut region = fixture(looping);
        verify(&region).unwrap();
        let before = format!("{region:?}");
        let blocks = region.blocks.as_ptr();
        let instructions = region.instructions.as_ptr();
        let values = region.values.as_ptr();
        let result = run(&mut region, Config::default()).unwrap();
        assert_eq!(result.loops, usize::from(looping));
        assert_eq!(result.hoisted, 0);
        assert!(result.work > 0);
        assert_eq!(region.blocks.as_ptr(), blocks);
        assert_eq!(region.instructions.as_ptr(), instructions);
        assert_eq!(region.values.as_ptr(), values);
        assert_eq!(format!("{region:?}"), before);
    }
}

#[test]
fn no_motion_fast_path_still_checks_input_and_work_budget() {
    let mut region = fixture(false);
    let before = format!("{region:?}");
    let no_work = Config { max_work: 0, ..Config::default() };
    assert!(run(&mut region, no_work).unwrap_err().contains("budget"));
    assert_eq!(format!("{region:?}"), before);
    region.instructions[0].block = BlockId(u32::MAX);
    let invalid = format!("{region:?}");
    assert!(run(&mut region, Config::default()).is_err());
    assert_eq!(format!("{region:?}"), invalid);
}

#[test]
fn metadata_limits_precede_verification_of_invalid_ids() {
    for kind in 0..11 {
        let mut region = fixture(false);
        let value = region.instructions[0].results[0];
        match kind {
            0 => region.blocks[0].instructions = vec![InstId(u32::MAX); MAX_METADATA_ITEMS + 1],
            1 => region.blocks[0].params = vec![value; MAX_METADATA_ITEMS + 1],
            2 => region.instructions[0].args = vec![value; MAX_METADATA_ITEMS + 1],
            3 => region.instructions[0].results = vec![value; MAX_METADATA_ITEMS + 1],
            4 => {
                region.blocks[0].terminator = Some(Terminator::Branch(Edge {
                    target: BlockId(0),
                    args: vec![value; MAX_METADATA_ITEMS + 1],
                }));
            }
            5 => region.states[0].xmm = vec![value; MAX_METADATA_ITEMS + 1],
            6 => region.states[0].x87 = vec![value; MAX_METADATA_ITEMS + 1],
            7 => region.helpers.push(crate::ir::helper::HelperDescriptor::conservative(
                "h".repeat(MAX_METADATA_ITEMS + 1),
                vec![],
                vec![],
            )),
            8 => region.helpers.push(crate::ir::helper::HelperDescriptor::conservative(
                "helper".into(),
                vec![Type::I32; MAX_METADATA_ITEMS + 1],
                vec![],
            )),
            9 => region.helpers.push(crate::ir::helper::HelperDescriptor::conservative(
                "helper".into(),
                vec![],
                vec![Type::I32; MAX_METADATA_ITEMS + 1],
            )),
            _ => {
                // Individually small vectors must share the same total cap.
                region.instructions[0].args = vec![value; MAX_METADATA_ITEMS / 2];
                region.instructions[0].results = vec![value; MAX_METADATA_ITEMS / 2];
            }
        }
        let before = format!("{region:?}");
        assert_eq!(
            run(&mut region, Config::default()).unwrap_err(),
            "LICM metadata budget exceeded"
        );
        assert_eq!(format!("{region:?}"), before);
    }
}
