//! Frontend construction of audited CPU helper calls.
use super::integer::IntegerBuilder;
use crate::ir::{
    helper::{HelperAbi, HelperDescriptor},
    hir::{Op, Terminator},
    ids::{HelperId, StateId, ValueId},
    types::Type,
};
pub(super) fn call(
    b: &mut IntegerBuilder,
    name: &str,
    args: Vec<ValueId>,
    state: StateId,
    terminal: bool,
) {
    call_abi(
        b,
        name,
        args,
        state,
        if terminal {
            HelperAbi::CpuExit
        } else {
            HelperAbi::Outcome {
                fault_delivery: None,
                normal_preserves_state: true,
            }
        },
    );
}
pub(super) fn call_abi(
    b: &mut IntegerBuilder,
    name: &str,
    args: Vec<ValueId>,
    state: StateId,
    abi: HelperAbi,
) {
    let terminal = matches!(abi, HelperAbi::CpuExit | HelperAbi::CpuRep);
    let mut descriptor = HelperDescriptor::conservative(
        name.into(),
        args.iter().map(|&a| b.ty(a)).collect(),
        vec![],
    );
    descriptor.abi = abi;
    let id = HelperId(b.region.helpers.len() as u32);
    b.region.helpers.push(descriptor);
    let mut args = args;
    args.push(b.effect);
    b.effect = b.region.append(
        b.block,
        Op::CallHelper(id),
        args,
        &[Type::Effect],
        Some(state),
    )[0];
    if terminal {
        b.region.terminate(b.block, Terminator::Exit(state));
    }
}
