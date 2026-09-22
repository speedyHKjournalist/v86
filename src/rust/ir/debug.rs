//! Opt-in compiler inspection. Copies are bounded and never participate in
//! publication, execution, snapshots or guest state.
use std::{collections::VecDeque, sync::Mutex};
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum VerifyMode {
    Off,
    #[default]
    Debug,
    EveryPass,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DumpMode {
    #[default]
    Off,
    Hir,
    Mir,
    Wasm,
    All,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Config {
    pub verify: VerifyMode,
    pub dump: DumpMode,
}
impl Config {
    pub fn from_raw(verify: u32, dump: u32) -> Option<Self> {
        Some(Self {
            verify: match verify {
                0 => VerifyMode::Off,
                1 => VerifyMode::Debug,
                2 => VerifyMode::EveryPass,
                _ => return None,
            },
            dump: match dump {
                0 => DumpMode::Off,
                1 => DumpMode::Hir,
                2 => DumpMode::Mir,
                3 => DumpMode::Wasm,
                4 => DumpMode::All,
                _ => return None,
            },
        })
    }
    pub fn hir(self) -> bool { matches!(self.dump, DumpMode::Hir | DumpMode::All) }
    pub fn mir(self) -> bool { matches!(self.dump, DumpMode::Mir | DumpMode::All) }
    pub fn wasm(self) -> bool { matches!(self.dump, DumpMode::Wasm | DumpMode::All) }
    pub fn check(
        self,
        mir: &super::mir::MirRegion,
        boundary: bool,
    ) -> Result<(), super::lowering::CompileError> {
        if self.verify == VerifyMode::EveryPass
            || boundary && self.verify == VerifyMode::Debug && cfg!(debug_assertions)
        {
            mir.verify()?;
        }
        Ok(())
    }
}
struct Record {
    pc: u32,
    tier: u32,
    hir: String,
    mir: String,
    wasm: Vec<u8>,
    truncated: u32,
}
const CAPACITY: usize = 16;
const TEXT_LIMIT: usize = 65536;
const WASM_LIMIT: usize = 262144;
static RECORDS: Mutex<VecDeque<Record>> = Mutex::new(VecDeque::new());
fn bounded(mut text: String, truncated: &mut u32, bit: u32) -> String {
    if text.len() > TEXT_LIMIT {
        let mut end = TEXT_LIMIT;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
        *truncated |= bit;
    }
    text
}
pub(crate) fn record(pc: u32, tier: u32, hir: String, mir: String, wasm: &[u8]) {
    let mut truncated = 0;
    let hir = bounded(hir, &mut truncated, 1);
    let mir = bounded(mir, &mut truncated, 2);
    if wasm.len() > WASM_LIMIT {
        truncated |= 4;
    }
    let wasm = wasm[..wasm.len().min(WASM_LIMIT)].to_vec();
    let mut records = RECORDS.try_lock().unwrap();
    if records.len() == CAPACITY {
        records.pop_front();
    }
    records.push_back(Record {
        pc,
        tier,
        hir,
        mir,
        wasm,
        truncated,
    });
}
#[no_mangle]
pub fn ir_dump_count() -> u32 { RECORDS.try_lock().unwrap().len() as u32 }
#[no_mangle]
pub fn ir_dump_clear() { RECORDS.try_lock().unwrap().clear(); }
/// Pointers are only valid until the next compiler call or clear. JS copies all
/// fields synchronously before yielding; Worker RPC returns those copies.
#[no_mangle]
pub fn ir_dump_info(index: u32, field: u32) -> u32 {
    let records = RECORDS.try_lock().unwrap();
    let Some(r) = records.get(index as usize)
    else {
        return 0;
    };
    match field {
        0 => r.pc,
        1 => r.tier,
        2 => r.hir.as_ptr() as u32,
        3 => r.hir.len() as u32,
        4 => r.mir.as_ptr() as u32,
        5 => r.mir.len() as u32,
        6 => r.wasm.as_ptr() as u32,
        7 => r.wasm.len() as u32,
        8 => r.truncated,
        _ => 0,
    }
}
