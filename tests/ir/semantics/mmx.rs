use crate::ir::{
    backend::wasm::emit_cpu,
    frontend::{
        decode::{GuestEip, LinearAddress},
        lift::{lift, lift_cpu},
        mmx::OPERATIONS,
    },
    lowering::lower,
    passes::{run, PassConfig},
};
#[test]
fn mmx_fixtures() {
    std::fs::create_dir_all("build/ir-mmx").unwrap();
    let mut cases = Vec::new();
    for &(key, width, forms) in OPERATIONS {
        let opcode = key & 0xFFFFFF;
        for mode in [false, true] {
            for address32 in [false, true] {
                for encoded_memory in [false, true] {
                    if forms & (if encoded_memory { 2 } else { 5 }) == 0 {
                        continue;
                    }
                    let memory = encoded_memory || opcode == 0x0FF7;
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
                        if forms != 4 {
                            let group = if key >> 24 != 0 { (key >> 24) as u8 } else { 1 };
                            bytes.push(
                                group << 3
                                    | if encoded_memory {
                                        if address32 {
                                            5
                                        }
                                        else {
                                            6
                                        }
                                    }
                                    else {
                                        0xC0 | if address32 { 0 } else { 1 }
                                    },
                            );
                        }
                        if encoded_memory {
                            bytes.extend_from_slice(
                                &0x6000u32.to_le_bytes()[..if address32 { 4 } else { 2 }],
                            );
                        }
                        if key >> 24 != 0 || [0x0F70, 0x0FC4, 0x0FC5].contains(&opcode) {
                            bytes.push(
                                [0, 1, 15, 16, 31, 32, 63, 255][usize::from(mode) * 4
                                    + usize::from(address32) * 2
                                    + usize::from(dirty)],
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
                                .is_err()
                        );
                        for opt in 0..2 {
                            if opt != 0 {
                                run(&mut r, PassConfig::default()).unwrap();
                            }
                            std::fs::write(
                                format!("build/ir-mmx/{}-{opt}.wasm", cases.len()),
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
    std::fs::write("build/ir-mmx/cases.json", format!("[{}]", cases.join(","))).unwrap();
}
