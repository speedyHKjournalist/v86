use crate::ir::{
    backend::wasm::emit_cpu_with_code_pages,
    frontend::{
        decode::{GuestEip, LinearAddress},
        lift::lift_cpu,
        region::lift_cpu_cfg,
    },
    lowering::lower,
    passes::{run, PassConfig},
};

const PC: u32 = 0x0010_0000;
const CODE_PAGE: u32 = PC & !4095;

#[test]
fn scalar_store_continuation_fixtures() {
    let cases: [(&str, &[u8]); 8] = [
        ("continue", &[0x88, 0x11, 0x43]),
        ("rmw_continue", &[0xFF, 0x01, 0x43]),
        ("rmw_fault", &[0xFF, 0x01, 0x8B, 0x06]),
        ("fault_after", &[0x88, 0x11, 0x8B, 0x06]),
        ("store_load", &[0x88, 0x11, 0x8A, 0x19]),
        ("rmw_load8", &[0xFE, 0x01, 0x8A, 0x19]),
        ("rmw_load16", &[0x66, 0xFF, 0x01, 0x66, 0x8B, 0x19]),
        ("rmw_load32", &[0xFF, 0x01, 0x8B, 0x19]),
    ];
    std::fs::create_dir_all("build/ir-store-continuation").unwrap();
    for (name, bytes) in cases {
        for cfg in [false, true] {
            for optimize in [false, true] {
                let mut region = if cfg {
                    lift_cpu_cfg(bytes, GuestEip(PC), LinearAddress(PC), true, 64)
                } else {
                    lift_cpu(bytes, GuestEip(PC), LinearAddress(PC), true)
                }
                .unwrap();
                if optimize {
                    run(&mut region, PassConfig::default()).unwrap();
                }
                let mut mir = lower(&region).unwrap();
                if optimize {
                    let forwarded = mir
                        .forward_ram_reads(crate::ir::mir::forwarding::DEFAULT_WORK_LIMIT)
                        .unwrap();
                    if !cfg {
                        assert_eq!(forwarded, usize::from(name == "store_load" || name.starts_with("rmw_load")));
                    }
                }
                let artifact = emit_cpu_with_code_pages(&mir, 100, &[CODE_PAGE]).unwrap();
                std::fs::write(
                    format!(
                        "build/ir-store-continuation/{name}-{}.wasm",
                        u8::from(optimize) + 2 * u8::from(cfg)
                    ),
                    artifact.bytes,
                )
                .unwrap();
            }
        }
    }
}

#[test]
fn scalar_store_continuation_requires_valid_code_pages() {
    let region = lift_cpu(&[0x88, 0x11, 0x43], GuestEip(PC), LinearAddress(PC), true).unwrap();
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
