use crate::ir::{
    backend::wasm::emit_cpu,
    frontend::{
        decode::{decode, GuestEip, LinearAddress},
        lift::{lift, lift_cpu},
    },
    helper::HelperAbi,
    lowering::lower,
    passes::{run, PassConfig},
};
#[test]
fn control_regs_fixtures() {
    std::fs::create_dir_all("build/ir-control-regs").unwrap();
    let mut cases = Vec::new();
    for mode in [false, true] {
        for op in 0x20..=0x23 {
            for index in 0..8 {
                for reg in 0..8 {
                    for prefix in [
                        vec![],
                        vec![0x66],
                        vec![0x67],
                        vec![0xF2],
                        vec![0xF3],
                        vec![0x64],
                        vec![0x66, 0x67, 0xF2, 0x64],
                    ] {
                        for m in 0..4 {
                            if !prefix.is_empty() && m != 3 {
                                continue;
                            }
                            // INC EBX observes a preceding FLAGS and register change, including when
                            // EBX is the implicit helper's data source or destination.
                            let mut bytes = vec![0x43];
                            bytes.extend(&prefix);
                            bytes.extend([0x0F, op, m << 6 | index << 3 | reg]);
                            let decoded =
                                decode(&bytes[1..], GuestEip(0x8001), LinearAddress(0x8001), mode)
                                    .unwrap();
                            assert!(decoded.ea.is_none());
                            assert_eq!(decoded.length as usize, bytes.len() - 1);
                            let mut r =
                                lift_cpu(&bytes, GuestEip(0x8000), LinearAddress(0x8000), mode)
                                    .unwrap();
                            for opt in 0..2 {
                                if opt != 0 {
                                    run(&mut r, PassConfig::default()).unwrap();
                                }
                                std::fs::write(
                                    format!("build/ir-control-regs/{}-{opt}.wasm", cases.len()),
                                    emit_cpu(&lower(&r).unwrap(), 100).unwrap().bytes,
                                )
                                .unwrap();
                            }
                            cases.push(format!("[{:?},{mode},{op},{index},{reg},{m}]", bytes));
                        }
                    }
                }
            }
        }
    }
    std::fs::write(
        "build/ir-control-regs/cases.json",
        format!("[{}]", cases.join(",")),
    )
    .unwrap();
}
#[test]
fn control_regs_terminal_contract() {
    for op in 0x20..=0x23 {
        let bytes = [0x0F, op, 0xC0];
        assert!(lift(&bytes, GuestEip(0), LinearAddress(0), true).is_err());
        assert!(lift_cpu(&[0xF0, 0x0F, op, 0xC0], GuestEip(0), LinearAddress(0), true).is_err());
        assert!(lift_cpu(&[0x0F, op, 0xC0, 0x90], GuestEip(0), LinearAddress(0), true).is_err());
        let r = lift_cpu(&bytes, GuestEip(0), LinearAddress(0), true).unwrap();
        assert!(matches!(r.helpers[0].abi, HelperAbi::CpuExit));
        assert_eq!(r.helpers[0].params.len(), 2);
        assert!(r.helpers[0].results.is_empty());
    }
}
