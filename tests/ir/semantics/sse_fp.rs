use crate::ir::{
    backend::wasm::emit_cpu,
    frontend::{
        decode::{GuestEip, LinearAddress},
        lift::{lift, lift_cpu},
        sse_fp::OPERATIONS,
    },
    lowering::lower,
    passes::{run, PassConfig},
};
#[test]
fn sse_fp_fixtures() {
    std::fs::create_dir_all("build/ir-sse-fp").unwrap();
    let mut cases = Vec::new();
    for &(opcode, width) in OPERATIONS {
        for mode in [false, true] {
            for address32 in [false, true] {
                for memory in [false, true] {
                    for dirty in [false, true] {
                        let mut bytes = vec![0x46];
                        if dirty {
                            bytes.extend_from_slice(&[0x66, 0x0F, 0xEF, 0xC9]);
                        }
                        if mode != address32 {
                            bytes.push(0x67);
                        }
                        if opcode > 0xFFFF {
                            bytes.push((opcode >> 16) as u8);
                        }
                        bytes.extend_from_slice(&[0x0F, opcode as u8]);
                        bytes.push(
                            8 | if memory {
                                if address32 {
                                    5
                                }
                                else {
                                    6
                                }
                            }
                            else {
                                0xC0
                            },
                        );
                        if memory {
                            bytes.extend_from_slice(
                                &0x6000u32.to_le_bytes()[..if address32 { 4 } else { 2 }],
                            );
                        }
                        if opcode & 255 == 0xC2 {
                            bytes.push(
                                (u8::from(mode) * 4 + u8::from(address32) * 2 + u8::from(dirty))
                                    | 0xF8,
                            );
                        }
                        assert!(
                            lift(&bytes, GuestEip(0x8000), LinearAddress(0x8000), mode).is_err()
                        );
                        let mut r = lift_cpu(&bytes, GuestEip(0x8000), LinearAddress(0x8000), mode)
                            .unwrap();
                        let mut suffix = bytes.clone();
                        suffix.push(0x90);
                        assert!(
                            lift_cpu(&suffix, GuestEip(0x8000), LinearAddress(0x8000), mode)
                                .is_ok()
                        );
                        for opt in 0..2 {
                            if opt != 0 {
                                run(&mut r, PassConfig::default()).unwrap();
                            }
                            std::fs::write(
                                format!("build/ir-sse-fp/{}-{opt}.wasm", cases.len()),
                                emit_cpu(
                                    &{
                                        let mut mir = lower(&r).unwrap();
                                        if opt != 0 {
                                            mir.schedule_operand_stack(262_144).unwrap();
                                            mir.allocate_machine_locals(4_000_000).unwrap();
                                        }
                                        mir
                                    },
                                    100,
                                )
                                .unwrap()
                                .bytes,
                            )
                            .unwrap();
                        }
                        cases.push(format!(
                            "[{:?},{mode},{opcode},{dirty},{memory},{width}]",
                            bytes
                        ));
                    }
                }
            }
        }
    }
    std::fs::write(
        "build/ir-sse-fp/cases.json",
        format!("[{}]", cases.join(",")),
    )
    .unwrap();
}
