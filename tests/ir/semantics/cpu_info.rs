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
#[test]
fn cpu_info_fixtures() {
    std::fs::create_dir_all("build/ir-cpu-info").unwrap();
    let mut cases = Vec::new();
    for mode in [false, true] {
        for op in [0xA2, 0x30, 0x31, 0x32] {
            for prefix in [
                vec![],
                vec![0x66],
                vec![0x67],
                vec![0xF2],
                vec![0xF3],
                vec![0x64],
                vec![0x66, 0x67, 0xF2, 0x64],
            ] {
                let mut bytes = vec![0x46];
                bytes.extend(prefix);
                bytes.extend([0x0F, op]);
                let mut r =
                    lift_cpu(&bytes, GuestEip(0x8000), LinearAddress(0x8000), mode).unwrap();
                for opt in 0..2 {
                    if opt != 0 {
                        run(&mut r, PassConfig::default()).unwrap();
                    }
                    std::fs::write(
                        format!("build/ir-cpu-info/{}-{opt}.wasm", cases.len()),
                        emit_cpu(&lower(&r).unwrap(), 100).unwrap().bytes,
                    )
                    .unwrap();
                }
                cases.push(format!("[{:?},{mode},{op}]", bytes));
            }
        }
    }
    std::fs::write(
        "build/ir-cpu-info/cases.json",
        format!("[{}]", cases.join(",")),
    )
    .unwrap();
}
#[test]
fn cpu_info_terminal_contract() {
    for op in [0xA2, 0x30, 0x31, 0x32] {
        let bytes = [0x0F, op];
        assert!(lift(&bytes, GuestEip(0), LinearAddress(0), true).is_err());
        assert!(lift_cpu(&[0xF0, 0x0F, op], GuestEip(0), LinearAddress(0), true).is_err());
        assert!(lift_cpu(&[0x0F, op, 0x90], GuestEip(0), LinearAddress(0), true).is_err());
        let r = lift_cpu(&bytes, GuestEip(0), LinearAddress(0), true).unwrap();
        assert_eq!(r.helpers.len(), 1);
        assert!(matches!(r.helpers[0].abi, HelperAbi::CpuExit));
        assert!(r.helpers[0].results.is_empty());
    }
}
