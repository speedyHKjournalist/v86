//! Register integer semantics. Each arithmetic flag is an independent SSA source.
use crate::ir::{hir::*, ids::*, state::FlagState, types::Type};
pub struct IntegerBuilder {
    pub region: Region,
    pub block: BlockId,
    pub gpr: [ValueId; 8],
    pub flags: FlagState,
    pub effect: ValueId,
    pub xmm: Vec<ValueId>,
}
impl IntegerBuilder {
    pub fn new() -> Self {
        let mut region = Region::default();
        let block = region.block(true);
        let effect = region.param(block, Type::Effect);
        let gpr = std::array::from_fn(|r| {
            region.append(block, Op::ReadGpr(r as u8), vec![], &[Type::I32], None)[0]
        });
        let flags = region.append(block, Op::ReadFlags, vec![], &[Type::I32], None)[0];
        let raw_flags = region.append(block, Op::ReadRawFlags, vec![], &[Type::I32], None)[0];
        let raw_zero = region.append(
            block,
            Op::Extract { lsb: 6 },
            vec![raw_flags],
            &[Type::I1],
            None,
        )[0];
        let changes = region.append(block, Op::ReadFlagChanges, vec![], &[Type::I32], None)[0];
        let zero_is_lazy = region.append(
            block,
            Op::Extract { lsb: 6 },
            vec![changes],
            &[Type::I1],
            None,
        )[0];
        let last_op1 = region.append(block, Op::ReadFlagOperand, vec![], &[Type::I32], None)[0];
        let bits = std::array::from_fn(|i| {
            region.append(
                block,
                Op::Extract {
                    lsb: [0, 2, 4, 6, 7, 11][i],
                },
                vec![flags],
                &[Type::I1],
                None,
            )[0]
        });
        Self {
            region,
            block,
            effect,
            gpr,
            xmm: vec![],
            flags: FlagState {
                arithmetic: bits,
                system: flags,
                last_op1: Some(last_op1),
                raw_zero: Some(raw_zero),
                zero_is_lazy: Some(zero_is_lazy),
            },
        }
    }
    pub fn ty(&self, value: ValueId) -> Type {
        self.region.values[value.index()].ty
    }
    pub fn node(&mut self, op: Op, args: Vec<ValueId>, ty: Type) -> ValueId {
        self.region.append(self.block, op, args, &[ty], None)[0]
    }
    pub fn constant(&mut self, n: u32, ty: Type) -> ValueId {
        let bits = ty.bits().unwrap();
        let mask = if bits >= 32 { u32::MAX } else { (1u32 << bits) - 1 };
        self.node(Op::Const((n & mask) as u64), vec![], ty)
    }
    pub fn binary(&mut self, op: Binary, a: ValueId, b: ValueId) -> ValueId {
        let ty = if matches!(op, Binary::Eq | Binary::Ult | Binary::Slt) {
            Type::I1
        } else {
            self.ty(a)
        };
        self.node(Op::Binary(op), vec![a, b], ty)
    }
    pub fn extract(&mut self, value: ValueId, lsb: u8, ty: Type) -> ValueId {
        self.node(Op::Extract { lsb }, vec![value], ty)
    }
    pub fn read(&mut self, register: u8, width: u8) -> ValueId {
        let (r, lsb) = if width == 8 {
            (register & 3, if register >= 4 { 8 } else { 0 })
        } else {
            (register, 0)
        };
        if width == 32 {
            self.gpr[r as usize]
        } else {
            self.extract(self.gpr[r as usize], lsb, width_type(width))
        }
    }
    pub fn write(&mut self, register: u8, width: u8, value: ValueId) {
        let (r, lsb) = if width == 8 {
            (register & 3, if register >= 4 { 8 } else { 0 })
        } else {
            (register, 0)
        };
        self.gpr[r as usize] = if width == 32 {
            value
        } else {
            self.node(
                Op::Insert { lsb },
                vec![self.gpr[r as usize], value],
                Type::I32,
            )
        };
    }
    pub fn arithmetic(&mut self, group: u8, a: ValueId, b: ValueId) -> ValueId {
        let ty = self.ty(a);
        let width = ty.bits().unwrap();
        let sub = matches!(group, 3 | 5 | 7);
        let op = match group {
            0 | 2 => Binary::Add,
            1 => Binary::Or,
            3 | 5 | 7 => Binary::Sub,
            4 => Binary::And,
            6 => Binary::Xor,
            _ => unreachable!(),
        };
        if !matches!(group, 1 | 4 | 6) {
            self.flags.last_op1 = Some(if ty == Type::I32 {
                a
            } else {
                self.node(Op::Extend { signed: false }, vec![a], Type::I32)
            });
        }
        let first = self.binary(op, a, b);
        let result = if group == 2 || group == 3 {
            let carry = self.node(
                Op::Extend { signed: false },
                vec![self.flags.arithmetic[0]],
                ty,
            );
            self.binary(op, first, carry)
        } else {
            first
        };
        let zero = self.constant(0, ty);
        let cf;
        let af;
        let of;
        if matches!(group, 1 | 4 | 6) {
            cf = self.constant(0, Type::I1);
            of = cf;
            af = cf; // Baseline logical AF is zero.
        } else {
            let c1 = if sub {
                self.binary(Binary::Ult, a, b)
            } else {
                self.binary(Binary::Ult, first, a)
            };
            cf = if group == 2 || group == 3 {
                let c2 = if sub {
                    self.binary(Binary::Ult, first, result)
                } else {
                    self.binary(Binary::Ult, result, first)
                };
                self.binary(Binary::Or, c1, c2)
            } else {
                c1
            };
            let ab = self.binary(Binary::Xor, a, b);
            let ar = self.binary(Binary::Xor, a, result);
            let af_bits = self.binary(Binary::Xor, ab, result);
            af = self.extract(af_bits, 4, Type::I1);
            let rhs = if sub { ab } else { self.binary(Binary::Xor, b, result) };
            let overflow = self.binary(Binary::And, ar, rhs);
            of = self.extract(overflow, width - 1, Type::I1);
        }
        let zf = self.binary(Binary::Eq, result, zero);
        let sf = self.extract(result, width - 1, Type::I1);
        let mut parity = self.extract(result, 0, Type::I1);
        for bit in 1..8 {
            let v = self.extract(result, bit, Type::I1);
            parity = self.binary(Binary::Xor, parity, v);
        }
        let one = self.constant(1, Type::I1);
        let pf = self.binary(Binary::Xor, parity, one);
        self.flags.arithmetic = [cf, pf, af, zf, sf, of];
        self.flags.zero_is_lazy = Some(one);
        result
    }
    pub fn condition(&mut self, cc: u8) -> ValueId {
        let [cf, pf, _, zf, sf, of] = self.flags.arithmetic;
        let value = match cc >> 1 {
            0 => of,
            1 => cf,
            2 => zf,
            3 => self.binary(Binary::Or, cf, zf),
            4 => sf,
            5 => pf,
            6 => self.binary(Binary::Xor, sf, of),
            7 => {
                let ne = self.binary(Binary::Xor, sf, of);
                self.binary(Binary::Or, zf, ne)
            },
            _ => unreachable!(),
        };
        if cc & 1 == 0 {
            value
        } else {
            let one = self.constant(1, Type::I1);
            self.binary(Binary::Xor, value, one)
        }
    }
}
pub fn width_type(width: u8) -> Type {
    match width {
        8 => Type::I8,
        16 => Type::I16,
        32 => Type::I32,
        _ => unreachable!(),
    }
}
