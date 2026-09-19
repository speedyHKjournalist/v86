use crate::ir::{
    backend::wasm::emit_cpu,
    frontend::{
        decode::{GuestEip, LinearAddress},
        lift::{lift, lift_cpu},
    },
    helper::HelperAbi,
    lowering::lower,
    passes::{run, PassConfig},
};

fn valid(opcode: u32, group: u32, r: u32) -> bool {
    match opcode {
        0xD8 | 0xDC => true,
        0xD9 => match group {
            0 | 1 | 3 | 6 | 7 => true,
            2 => r == 0,
            4 => matches!(r, 0 | 1 | 4 | 5),
            5 => r <= 6,
            _ => false,
        },
        0xDA => match group {
            0..=3 => true,
            5 => r == 1,
            _ => false,
        },
        0xDB => match group {
            0..=3 | 5 | 6 => true,
            4 => r <= 4,
            _ => false,
        },
        0xDD => group <= 5,
        0xDE => group != 3 || r == 1,
        0xDF => match group {
            0..=3 | 5 | 6 => true,
            4 => r == 0,
            _ => false,
        },
        _ => false,
    }
}

#[test]
fn x87_register_fixtures() {
    std::fs::create_dir_all("build/ir-x87").unwrap();
    let mut cases = Vec::new();

    for opcode in 0xD8u32..=0xDF {
        let operand_prefixes: &[&[u8]] = if matches!(opcode, 0xD9 | 0xDD) {
            &[&[], &[0x66]]
        } else {
            &[&[]]
        };
        for &prefix in operand_prefixes {
            for group in 0u32..8 {
                for r in 0u32..8 {
                    // Dirty one GPR before the terminal helper so the fixture
                    // also proves pre-call StateMap materialization and exact
                    // instruction retirement.
                    let mut bytes = vec![0x46];
                    bytes.extend_from_slice(prefix);
                    bytes.push(opcode as u8);
                    bytes.push(0xC0 | (group << 3) as u8 | r as u8);

                    let mut region =
                        lift_cpu(&bytes, GuestEip(0x8000), LinearAddress(0x8000), true).unwrap();
                    assert_eq!(region.helpers.len(), 1);
                    assert_eq!(region.helpers[0].name, "ir_x87_reg");
                    assert!(matches!(region.helpers[0].abi, HelperAbi::CpuExit));

                    for opt in 0..2 {
                        if opt != 0 {
                            run(&mut region, PassConfig::default()).unwrap();
                        }
                        let mir = lower(&region).unwrap();
                        std::fs::write(
                            format!("build/ir-x87/{}-{opt}.wasm", cases.len()),
                            emit_cpu(&mir, 100).unwrap().bytes,
                        )
                        .unwrap();
                    }

                    cases.push(format!(
                        "[{:?},{opcode},{group},{r},{},{}]",
                        bytes,
                        valid(opcode, group, r),
                        !prefix.is_empty()
                    ));
                }
            }
        }
    }

    std::fs::write(
        "build/ir-x87/cases.json",
        format!("[{}]", cases.join(",")),
    )
    .unwrap();
}

#[test]
fn x87_register_terminal_contract() {
    for opcode in 0xD8u8..=0xDF {
        let bytes = [opcode, 0xC0];
        assert!(lift(&bytes, GuestEip(0), LinearAddress(0), true).is_err());
        assert!(lift_cpu(&[opcode, 0xC0, 0x90], GuestEip(0), LinearAddress(0), true).is_err());
        assert!(lift_cpu(&[0xF0, opcode, 0xC0], GuestEip(0), LinearAddress(0), true).is_err());

        let region = lift_cpu(&bytes, GuestEip(0), LinearAddress(0), true).unwrap();
        assert_eq!(region.helpers.len(), 1);
        assert_eq!(region.helpers[0].name, "ir_x87_reg");
        assert!(matches!(region.helpers[0].abi, HelperAbi::CpuExit));
    }

    // Address-size override is semantically inert for mod=3, but must remain
    // accepted by the shared decoder/frontend.
    assert!(lift_cpu(
        &[0x67, 0xD8, 0xC1],
        GuestEip(0),
        LinearAddress(0),
        true
    )
    .is_ok());
}
