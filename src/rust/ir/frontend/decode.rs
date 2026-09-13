//! Side-effect-free decoding over caller-owned code bytes. Never reads live CPU memory.
#[path = "encodings.rs"]
mod generated;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GuestEip(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LinearAddress(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PhysicalAddress(pub u32);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Prefixes {
    pub operand: bool,
    pub address: bool,
    pub lock: bool,
    // Baseline accumulates F2/F3 and chooses F2 first if both are present.
    pub repne: bool,
    pub rep: bool,
    pub segment: Option<u8>,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImmediateKind {
    None,
    Byte,
    SignedByte,
    Word,
    Operand,
    Address,
}
#[derive(Debug)]
pub struct Encoding {
    pub id: u32,
    pub opcode: u32,
    pub group: i8,
    pub immediate: ImmediateKind,
    pub extra_bytes: u8,
    // Legacy dispatch fetches ModRM before selecting the mandatory-prefix form.
    pub fetch_modrm: bool,
    pub custom: bool,
    pub e: bool,
    pub ignore_mod: bool,
    pub reg_ud: bool,
    pub mem_ud: bool,
    pub os: bool,
    pub block_boundary: bool,
    pub no_next_instruction: bool,
    pub absolute_jump: bool,
    pub jump_offset_imm: bool,
    pub conditional_jump: bool,
    pub custom_sti: bool,
    pub is_fpu: bool,
    pub sse: bool,
    pub is_string: bool,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EffectiveAddress {
    pub base: Option<u8>,
    pub index: Option<u8>,
    pub scale: u8,
    pub displacement: u32,
    pub address_size: u8,
    pub segment: u8,
}
impl EffectiveAddress {
    /// Offset arithmetic only. Segmentation and translation belong to runtime lowering.
    pub fn offset(&self, gpr: &[u32; 8]) -> u32 {
        let result = self
            .displacement
            .wrapping_add(self.base.map_or(0, |r| gpr[r as usize]))
            .wrapping_add(
                self.index
                    .map_or(0, |r| gpr[r as usize].wrapping_shl(self.scale as u32)),
            );
        if self.address_size == 16 {
            result & 0xFFFF
        } else {
            result
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Flow {
    Next,
    Boundary,
    Stop,
    Sti,
    Relative {
        displacement: i32,
        conditional: bool,
        call: bool,
    },
}
#[derive(Clone, Debug)]
pub struct DecodedInstruction {
    pub encoding: &'static Encoding,
    pub instruction_pc: GuestEip,
    pub next_pc: GuestEip,
    pub linear_pc: LinearAddress,
    pub bytes: [u8; 15],
    pub length: u8,
    pub operand_size: u8,
    pub address_size: u8,
    pub prefixes: Prefixes,
    pub modrm: Option<u8>,
    pub modrm_offset: Option<u8>,
    pub ea: Option<EffectiveAddress>,
    pub immediate: Option<u32>,
    pub extra_immediate: Option<u16>,
    pub baseline_ud: bool,
    pub flow: Flow,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DecodeStop {
    Incomplete { available: usize },
    TooLong,
    UnknownEncoding { opcode: u32 },
}

struct Cursor<'a> {
    bytes: &'a [u8],
    position: usize,
}
impl Cursor<'_> {
    fn read(&mut self) -> Result<u8, DecodeStop> {
        if self.position == 15 {
            return Err(DecodeStop::TooLong);
        }
        let byte = *self
            .bytes
            .get(self.position)
            .ok_or(DecodeStop::Incomplete {
                available: self.bytes.len(),
            })?;
        self.position += 1;
        Ok(byte)
    }
    fn integer(&mut self, bytes: u8) -> Result<u32, DecodeStop> {
        let mut value = 0;
        for shift in 0..bytes {
            value |= (self.read()? as u32) << (shift * 8);
        }
        Ok(value)
    }
}

pub fn encodings() -> &'static [Encoding] {
    generated::ENCODINGS
}
fn candidates(opcode: u32) -> &'static [Encoding] {
    match generated::OPCODES.binary_search_by_key(&opcode, |&(key, _, _)| key) {
        Ok(i) => {
            let (_, start, end) = generated::OPCODES[i];
            &generated::ENCODINGS[start..end]
        },
        Err(_) => &[],
    }
}

/// `bytes` is a logical contiguous instruction snapshot; it may span physical pages.
/// Missing bytes return a compile stop. The caller must deliver fetch faults during execution.
pub fn decode(
    bytes: &[u8],
    pc: GuestEip,
    linear: LinearAddress,
    default_32: bool,
) -> Result<DecodedInstruction, DecodeStop> {
    let mut c = Cursor { bytes, position: 0 };
    let mut prefixes = Prefixes::default();
    let first = loop {
        let byte = c.read()?;
        match byte {
            0x26 => prefixes.segment = Some(0),
            0x2E => prefixes.segment = Some(1),
            0x36 => prefixes.segment = Some(2),
            0x3E => prefixes.segment = Some(3),
            0x64 => prefixes.segment = Some(4),
            0x65 => prefixes.segment = Some(5),
            0x66 => prefixes.operand = true,
            0x67 => prefixes.address = true,
            0xF0 => prefixes.lock = true,
            0xF2 => prefixes.repne = true,
            0xF3 => prefixes.rep = true,
            _ => break byte,
        }
    };
    let base_opcode = if first == 0x0F { 0x0F00 | c.read()? as u32 } else { first as u32 };
    let operand_size = if default_32 != prefixes.operand { 32 } else { 16 };
    let address_size = if default_32 != prefixes.address { 32 } else { 16 };
    let mut opcode = base_opcode;
    // Match the fixed baseline's mandatory-prefix precedence, including ignored prefixes.
    for (present, prefix) in [
        (first == 0x0F && prefixes.operand, 0x66),
        (prefixes.repne, 0xF2),
        (prefixes.rep, 0xF3),
    ] {
        let prefixed = prefix << (if first == 0x0F { 16 } else { 8 }) | base_opcode;
        if present && !candidates(prefixed).is_empty() {
            opcode = prefixed;
            break;
        }
    }
    let rows = candidates(opcode);
    let first = rows.first().ok_or(DecodeStop::UnknownEncoding { opcode })?;
    let modrm_offset = if first.fetch_modrm { Some(c.position as u8) } else { None };
    let modrm = if first.fetch_modrm { Some(c.read()?) } else { None };
    let encoding = rows
        .iter()
        .find(|row| row.group < 0 || modrm.map(|m| (m >> 3 & 7) as i8) == Some(row.group))
        .ok_or(DecodeStop::UnknownEncoding { opcode })?;
    let ea = match modrm {
        Some(m) if encoding.e && m < 0xC0 && !encoding.ignore_mod => {
            Some(decode_ea(&mut c, m, address_size, prefixes.segment)?)
        },
        _ => None,
    };
    let immediate = match encoding.immediate {
        ImmediateKind::None => None,
        ImmediateKind::Byte => Some(c.integer(1)?),
        ImmediateKind::SignedByte => Some(c.read()? as i8 as i32 as u32),
        ImmediateKind::Word => Some(c.integer(2)?),
        ImmediateKind::Operand => Some(c.integer(operand_size / 8)?),
        ImmediateKind::Address => Some(c.integer(address_size / 8)?),
    };
    let extra_immediate =
        if encoding.extra_bytes > 0 { Some(c.integer(encoding.extra_bytes)? as u16) } else { None };
    let baseline_ud = if ea.is_some() { encoding.mem_ud } else { encoding.reg_ud };
    let flow = if encoding.jump_offset_imm {
        Flow::Relative {
            displacement: immediate.unwrap() as i32,
            conditional: encoding.conditional_jump,
            call: base_opcode == 0xE8,
        }
    } else if encoding.custom_sti {
        Flow::Sti
    } else if encoding.no_next_instruction {
        Flow::Stop
    } else if encoding.block_boundary || !encoding.custom && encoding.e || baseline_ud {
        Flow::Boundary
    } else {
        Flow::Next
    };
    let mut raw = [0; 15];
    raw[..c.position].copy_from_slice(&bytes[..c.position]);
    Ok(DecodedInstruction {
        encoding,
        instruction_pc: pc,
        next_pc: GuestEip(pc.0.wrapping_add(c.position as u32)),
        linear_pc: linear,
        bytes: raw,
        length: c.position as u8,
        operand_size,
        address_size,
        prefixes,
        modrm,
        modrm_offset,
        ea,
        immediate,
        extra_immediate,
        baseline_ud,
        flow,
    })
}

fn decode_ea(
    c: &mut Cursor<'_>,
    modrm: u8,
    size: u8,
    segment: Option<u8>,
) -> Result<EffectiveAddress, DecodeStop> {
    let mode = modrm >> 6;
    let rm = modrm & 7;
    let (base, index, scale, absolute) = if size == 16 {
        let (base, index) = match rm {
            0 => (Some(3), Some(6)),
            1 => (Some(3), Some(7)),
            2 => (Some(5), Some(6)),
            3 => (Some(5), Some(7)),
            4 => (Some(6), None),
            5 => (Some(7), None),
            6 if mode == 0 => (None, None),
            6 => (Some(5), None),
            _ => (Some(3), None),
        };
        (base, index, 0, mode == 0 && rm == 6)
    } else if rm == 4 {
        let sib = c.read()?;
        let b = sib & 7;
        let i = sib >> 3 & 7;
        (
            if b == 5 && mode == 0 { None } else { Some(b) },
            if i == 4 { None } else { Some(i) },
            sib >> 6,
            b == 5 && mode == 0,
        )
    } else {
        (
            if rm == 5 && mode == 0 { None } else { Some(rm) },
            None,
            0,
            rm == 5 && mode == 0,
        )
    };
    let displacement = if mode == 1 {
        c.read()? as i8 as i32 as u32
    } else if mode == 2 || absolute {
        c.integer(size / 8)?
    } else {
        0
    };
    Ok(EffectiveAddress {
        base,
        index,
        scale,
        displacement,
        address_size: size,
        segment: segment.unwrap_or(if base == Some(5) || base == Some(4) && size == 32 {
            2
        } else {
            3
        }),
    })
}

#[cfg(test)]
#[path = "../../../../tests/ir/decode/decode.rs"]
mod tests;
