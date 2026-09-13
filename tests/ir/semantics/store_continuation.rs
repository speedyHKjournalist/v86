use crate::ir::{
    backend::wasm::emit_cpu_with_code_pages,
    frontend::{
        decode::{GuestEip, LinearAddress},
        lift::lift_cpu,
    },
    lowering::lower,
    passes::{run, PassConfig},
};

const PC: u32 = 0x0010_0000;
const CODE_PAGE: u32 = PC & !4095;

#[test]
fn scalar_store_continuation_fixtures() {
    let cases: [(&str, &[u8]); 2] = [
        ("continue", &[0x88, 0x11, 0x43]),
        ("fault_after", &[0x88, 0x11, 0x8B, 0x06]),
    ];
    std::fs::create_dir_all("build/ir-store-continuation").unwrap();
    for (name, bytes) in cases {
        for optimize in [false, true] {
            let mut region = lift_cpu(
                bytes,
                GuestEip(PC),
                LinearAddress(PC),
                true,
            )
            .unwrap();
            if optimize {
                run(&mut region, PassConfig::default()).unwrap();
            }
            let mir = lower(&region).unwrap();
            let artifact = emit_cpu_with_code_pages(&mir, 100, &[CODE_PAGE]).unwrap();
            std::fs::write(
                format!("build/ir-store-continuation/{name}-{}.wasm", u8::from(optimize)),
                artifact.bytes,
            )
            .unwrap();
        }
    }
}

#[test]
fn scalar_store_continuation_requires_valid_code_pages() {
    let region = lift_cpu(
        &[0x88, 0x11, 0x43],
        GuestEip(PC),
        LinearAddress(PC),
        true,
    )
    .unwrap();
    let mir = lower(&region).unwrap();
    assert!(emit_cpu_with_code_pages(&mir, 100, &[CODE_PAGE + 1]).is_err());
    assert!(emit_cpu_with_code_pages(&mir, 100, &[CODE_PAGE, CODE_PAGE]).is_err());
    assert!(emit_cpu_with_code_pages(
        &mir,
        100,
        &[
            0x0000_0000,
            0x0000_1000,
            0x0000_2000,
            0x0000_3000,
            0x0000_4000,
            0x0000_5000,
            0x0000_6000,
            0x0000_7000,
            0x0000_8000,
        ],
    )
    .is_err());
}
