use super::frontend::decode::GuestEip;
use super::ids::ValueId;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResumeKind {
    BeforeInstruction,
    AfterInstruction,
    RepProgress,
}
#[derive(Clone, Debug)]
pub struct FlagState {
    /// CF, PF, AF, ZF, SF, OF. Each source is an I1 SSA value (or an extracted input bit).
    pub arithmetic: [ValueId; 6],
    pub system: ValueId,
    /// Pinned CPU provenance read by undefined AF after shifts.
    pub last_op1: Option<ValueId>,
    /// Raw CPU ZF backing bit, observed by VERR/VERW before descriptor lookup.
    /// None requests ordinary architectural materialization (standalone states).
    pub raw_zero: Option<ValueId>,
    /// Whether CPU getzf uses the computed value instead of the backing bit.
    pub zero_is_lazy: Option<ValueId>,
}
#[derive(Clone, Debug)]
pub struct StateMap {
    pub instruction_pc: GuestEip,
    pub next_pc: GuestEip,
    /// Optional SSA CS-relative destination for a completed control transfer.
    pub next_value: Option<ValueId>,
    pub resume: ResumeKind,
    pub gpr: [ValueId; 8],
    pub flags: FlagState,
    pub xmm: Vec<ValueId>,
    pub x87: Vec<ValueId>,
    pub committed_instructions: u32,
    /// Dynamic completed-work base relative to this entry; the static field is an offset.
    pub count_base: Option<ValueId>,
    pub rep_progress: Option<[ValueId; 3]>, // ECX, ESI, EDI
}
impl StateMap {
    pub fn values(&self) -> Vec<ValueId> {
        let mut values = self.gpr.to_vec();
        values.extend(self.flags.arithmetic);
        values.push(self.flags.system);
        values.extend(self.flags.last_op1);
        values.extend(self.flags.raw_zero);
        values.extend(self.flags.zero_is_lazy);
        values.extend(&self.xmm);
        values.extend(&self.x87);
        values.extend(self.next_value);
        values.extend(self.count_base);
        if let Some(rep) = self.rep_progress {
            values.extend(rep);
        }
        values
    }
}
