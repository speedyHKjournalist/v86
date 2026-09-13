//! Ordered state writes and count phases selected before Wasm emission.
use super::value::{self, Address, Load, Reading, Scalar, Step};
use crate::ir::{
    hir::Region,
    lowering::CompileError,
    state::{ResumeKind, StateMap},
};
use crate::wasmgen::wasm_builder::WasmType;
use crate::{cpu::global_pointers as gp, regs::CS};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Store {
    I32,
    V128,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Write {
    pub address: Address,
    pub store: Store,
    pub expression: Vec<Step>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CountMode {
    Absolute,
    Delta,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Count {
    pub destination: Address,
    pub snapshot: u32,
    pub base: Option<crate::ir::ids::ValueId>,
    pub mode: CountMode,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Materialization {
    pub writes: Vec<Write>,
    pub count: Count,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatePlan {
    pub cpu: Materialization,
    pub standalone: Materialization,
    pub decoded_next: Write,
    pub requires_cpu: bool,
}
fn write(address: Address, expression: Vec<Step>) -> Write {
    Write {
        address,
        store: Store::I32,
        expression,
    }
}
fn cs_base() -> Step {
    let read = Reading::Memory {
        address: Address::Absolute(gp::get_seg_offset(CS)),
        load: Load::I32,
    };
    Step::Read {
        cpu: read.clone(),
        standalone: read,
    }
}
fn target(state: &StateMap, cpu: bool) -> Materialization {
    use Step::{Value, I32};
    let mut writes: Vec<_> = state
        .gpr
        .iter()
        .enumerate()
        .map(|(reg, &value)| write(Address::Gpr(reg as u8), vec![Value(value)]))
        .collect();
    for (reg, &value) in state.xmm.iter().enumerate() {
        writes.push(Write {
            address: Address::Absolute(gp::get_reg_xmm_offset(reg as u32)),
            store: Store::V128,
            expression: vec![Value(value)],
        });
    }
    if let Some(value) = state.flags.last_op1 {
        writes.push(write(Address::FlagOperand, vec![Value(value)]));
    }
    let mut flags = vec![
        Value(state.flags.system),
        I32(!0x8D5),
        Step::Scalar(Scalar::I32And),
    ];
    let raw_zero = if cpu { state.flags.raw_zero } else { None };
    for (&value, shift) in state.flags.arithmetic.iter().zip([0, 2, 4, 6, 7, 11]) {
        flags.extend([
            Value(if shift == 6 { raw_zero.unwrap_or(value) } else { value }),
            I32(shift),
            Step::Scalar(Scalar::I32Shl),
            Step::Scalar(Scalar::I32Or),
        ]);
    }
    writes.push(write(Address::Flags, flags));
    if cpu {
        if raw_zero.is_some() {
            writes.push(write(
                Address::Absolute(gp::last_result as u32),
                vec![
                    Value(state.flags.arithmetic[3]),
                    I32(1),
                    Step::Scalar(Scalar::I32Xor),
                ],
            ));
            writes.push(write(
                Address::Absolute(gp::last_op_size as u32),
                vec![I32(31)],
            ));
        }
        let lazy = if let Some(value) = state.flags.zero_is_lazy {
            vec![Value(value), I32(6), Step::Scalar(Scalar::I32Shl)]
        } else {
            vec![I32(0)]
        };
        writes.push(write(Address::Absolute(gp::flags_changed as u32), lazy));
        writes.push(write(
            Address::Absolute(gp::previous_ip as u32),
            vec![
                cs_base(),
                I32(state.instruction_pc.0 as i32),
                Step::Scalar(Scalar::I32Add),
            ],
        ));
    }
    let mut eip = vec![if let Some(value) = state.next_value {
        Value(value)
    } else {
        I32(if state.resume == ResumeKind::AfterInstruction {
            state.next_pc.0
        } else {
            state.instruction_pc.0
        } as i32)
    }];
    if cpu {
        eip.extend([cs_base(), Step::Scalar(Scalar::I32Add)]);
    }
    writes.push(write(Address::Eip, eip));
    Materialization {
        writes,
        count: Count {
            destination: Address::Committed,
            snapshot: state.committed_instructions,
            base: state.count_base,
            mode: if cpu { CountMode::Delta } else { CountMode::Absolute },
        },
    }
}
pub fn lower(state: &StateMap) -> StatePlan {
    StatePlan {
        cpu: target(state, true),
        standalone: target(state, false),
        requires_cpu: !state.xmm.is_empty(),
        decoded_next: write(
            Address::Absolute(gp::instruction_pointer as u32),
            vec![
                cs_base(),
                Step::I32(state.next_pc.0 as i32),
                Step::Scalar(Scalar::I32Add),
            ],
        ),
    }
}
pub fn verify(region: &Region, plans: &[StatePlan]) -> Result<(), CompileError> {
    if plans.len() != region.states.len()
        || region.states.iter().zip(plans).any(|(s, p)| lower(s) != *p)
    {
        return Err(CompileError::InvalidIr(
            "invalid state materialization plan".into(),
        ));
    }
    for plan in plans {
        for count in [&plan.cpu.count, &plan.standalone.count] {
            if let Some(base) = count.base {
                value::verify_expression(region, &[Step::Value(base)], WasmType::I32)?;
            }
        }
        for write in plan
            .cpu
            .writes
            .iter()
            .chain(&plan.standalone.writes)
            .chain(std::iter::once(&plan.decoded_next))
        {
            value::verify_expression(
                region,
                &write.expression,
                match write.store {
                    Store::I32 => WasmType::I32,
                    Store::V128 => WasmType::V128,
                },
            )?;
        }
    }
    Ok(())
}
