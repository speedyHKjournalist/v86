//! Basic blocks of one guest code page, discovered from its observed entries.
//!
//! Blocks are straight-line runs of complete instructions inside the page.
//! A block ends after a control transfer, a block-boundary encoding (mode,
//! paging and I/O instructions) or an instruction without a template (both
//! run in the interpreter), or before an instruction that cannot be decoded
//! within the page. Blocks may share bytes when a branch enters the
//! middle of another block's run; each is decoded from its own start.
use crate::ir::frontend::decode::{decode, DecodedInstruction, Flow, GuestEip, LinearAddress};
use std::collections::BTreeSet;

pub const PAGE: usize = 4096;
/// Bounded work per page, and a function size V8's optimizing tier handles
/// comfortably (the legacy JIT caps a module at about 250 extra blocks).
const MAX_BLOCKS: usize = 256;
const MAX_INSTRUCTIONS: usize = 2048;

pub struct Instruction {
    pub offset: u16,
    pub decoded: DecodedInstruction,
}
impl Instruction {
    pub fn end(&self) -> usize { self.offset as usize + self.decoded.length as usize }
}
pub struct Block {
    pub start: u16,
    pub instructions: Vec<Instruction>,
}
impl Block {
    /// Offset after the last instruction: the static fallthrough.
    pub fn end(&self) -> usize { self.instructions.last().map_or(self.start as usize, Instruction::end) }
}
pub struct PagePlan {
    pub blocks: Vec<Block>,
    /// Block index for every offset that starts a block.
    pub block_at: Vec<Option<u32>>,
    /// Blocks that start where a CALL of this page ends.
    pub return_site: Vec<bool>,
    /// Static targets (and fall-throughs) outside the analyzed bytes, as
    /// offsets from their start (negative: before it).
    pub external: Vec<i64>,
}

/// Direct successors of a block-ending instruction, as offsets from base_pc
/// (possibly outside the analyzed bytes).
fn successors(i: &DecodedInstruction, offset: usize, base_pc: GuestEip) -> Vec<i64> {
    let end = offset + i.length as usize;
    let mut out = vec![];
    if let Flow::Relative { displacement, conditional, call } = i.flow {
        let target = i.next_pc.0.wrapping_add(displacement as u32);
        let target = if i.operand_size == 16 { target & 0xFFFF } else { target };
        out.push(target.wrapping_sub(base_pc.0) as i32 as i64);
        // The return site is dispatched by EIP after the callee's RET.
        if conditional || call {
            out.push(end as i64);
        }
    }
    out
}
/// Control transfers, boundary encodings, and instructions without a
/// template: those run in the interpreter and continue by dispatch.
fn ends_block(i: &DecodedInstruction) -> bool {
    i.baseline_ud
        || i.encoding.block_boundary
        || !matches!(i.flow, Flow::Next | Flow::Boundary)
        || !super::emit::templated(i)
}

pub fn analyze(
    bytes: &[u8],
    base_pc: GuestEip,
    base_linear: LinearAddress,
    default_32: bool,
    entries: &[usize],
    // Block starts in this range are decoded first (the budget favors them).
    prefer: std::ops::Range<usize>,
) -> PagePlan {
    let size = bytes.len();
    debug_assert!(size % PAGE == 0 && size <= 3 * PAGE);
    let mut blocks: Vec<Block> = vec![];
    let mut block_at = vec![None; size];
    let mut external = vec![];
    let mut pending: BTreeSet<usize> = entries.iter().copied().filter(|&at| at < size).collect();
    let mut total = 0;
    let next = |pending: &mut BTreeSet<usize>| {
        let preferred = pending.range(prefer.clone()).next().copied();
        preferred.map(|at| pending.take(&at).unwrap()).or_else(|| pending.pop_first())
    };
    while let Some(start) = next(&mut pending) {
        if block_at[start].is_some() || blocks.len() == MAX_BLOCKS || total >= MAX_INSTRUCTIONS {
            continue;
        }
        let mut instructions = vec![];
        let mut at = start;
        loop {
            let Ok(decoded) = decode(
                &bytes[at..],
                GuestEip(base_pc.0.wrapping_add(at as u32)),
                LinearAddress(base_linear.0.wrapping_add(at as u32)),
                default_32,
            )
            else {
                break; // undecodable or crosses the page end: leave to the interpreter
            };
            let length = decoded.length as usize;
            let last = ends_block(&decoded);
            if last {
                for target in successors(&decoded, at, base_pc) {
                    if (0..size as i64).contains(&target) {
                        pending.insert(target as usize);
                    }
                    else {
                        external.push(target);
                    }
                }
                // Interpreted instructions continue at their fall-through.
                if matches!(decoded.flow, Flow::Next | Flow::Boundary) {
                    if at + length < size {
                        pending.insert(at + length);
                    }
                    else {
                        external.push((at + length) as i64);
                    }
                }
            }
            instructions.push(Instruction { offset: at as u16, decoded });
            total += 1;
            at += length;
            // Stop at the page end or where another block already starts.
            if last || at >= size || block_at[at].is_some() || total >= MAX_INSTRUCTIONS {
                if !last && at < size {
                    pending.insert(at);
                }
                if !last && at >= size {
                    external.push(at as i64);
                }
                break;
            }
        }
        if instructions.is_empty() {
            continue;
        }
        block_at[start] = Some(blocks.len() as u32);
        blocks.push(Block { start: start as u16, instructions });
    }
    // Address order: a jump to a higher address is a forward branch in the
    // emitted function (see emit::Page::goto_linear).
    blocks.sort_by_key(|b| b.start);
    let mut block_at = vec![None; size];
    for (k, b) in blocks.iter().enumerate() {
        block_at[b.start as usize] = Some(k as u32);
    }
    let mut return_site = vec![false; blocks.len()];
    for b in &blocks {
        let last = &b.instructions.last().unwrap().decoded;
        let call = matches!(last.flow, Flow::Relative { call: true, .. })
            || last.encoding.opcode == 0xFF && last.modrm.is_some_and(|m| m >> 3 & 7 == 2);
        if let Some(Some(k)) = block_at.get(b.end()).copied().filter(|_| call) {
            return_site[k as usize] = true;
        }
    }
    PagePlan { blocks, block_at, return_site, external }
}

/// Structured layout of a page function: blocks in address order, with each
/// cycle of the block graph (a strongly connected set) as a Wasm loop whose
/// first block, its header, is the member with the lowest address. Nested
/// cycles (those left after removing the edges into the header) nest.
pub enum Unit {
    Block(u32),
    Loop { header: u32, units: Vec<Unit> },
}
impl Unit {
    pub fn blocks(&self, out: &mut Vec<u32>) {
        match self {
            Unit::Block(k) => out.push(*k),
            Unit::Loop { units, .. } => units.iter().for_each(|u| u.blocks(out)),
        }
    }
    fn first(&self) -> u32 {
        match self {
            Unit::Block(k) => *k,
            Unit::Loop { header, .. } => *header,
        }
    }
}

/// In-page successors of every block (static edges only: dynamic targets
/// such as returns reach blocks through the dispatcher).
fn block_successors(plan: &PagePlan) -> Vec<Vec<u32>> {
    plan.blocks
        .iter()
        .map(|b| {
            let last = b.instructions.last().unwrap();
            let base = GuestEip(last.decoded.instruction_pc.0.wrapping_sub(last.offset as u32));
            let mut targets = successors(&last.decoded, last.offset as usize, base);
            if matches!(last.decoded.flow, Flow::Next | Flow::Boundary) {
                targets.push(b.end() as i64);
            }
            targets
                .into_iter()
                .filter_map(|at| usize::try_from(at).ok().and_then(|at| plan.block_at.get(at).copied().flatten()))
                .collect()
        })
        .collect()
}

pub fn layout(plan: &PagePlan) -> Vec<Unit> {
    let successors = block_successors(plan);
    let all: Vec<u32> = (0..plan.blocks.len() as u32).collect();
    units(&all, &successors, None)
}

/// Units of `members` (sorted), ignoring edges into `header`.
fn units(members: &[u32], successors: &[Vec<u32>], header: Option<u32>) -> Vec<Unit> {
    let n = successors.len();
    let mut member = vec![false; n];
    for &k in members {
        member[k as usize] = true;
    }
    let edges = |k: u32| -> Vec<u32> {
        successors[k as usize]
            .iter()
            .copied()
            .filter(|&t| member[t as usize] && Some(t) != header)
            .collect()
    };
    // Tarjan's strongly connected components.
    let mut index = vec![u32::MAX; n];
    let mut low = vec![0u32; n];
    let mut on_stack = vec![false; n];
    let mut stack = vec![];
    let mut next = 0;
    let mut components: Vec<Vec<u32>> = vec![];
    for &root in members {
        if index[root as usize] != u32::MAX {
            continue;
        }
        // Iterative DFS: (block, successor position).
        let mut work = vec![(root, 0usize)];
        index[root as usize] = next;
        low[root as usize] = next;
        next += 1;
        stack.push(root);
        on_stack[root as usize] = true;
        while let Some(&mut (v, ref mut position)) = work.last_mut() {
            let targets = edges(v);
            if let Some(&w) = targets.get(*position) {
                *position += 1;
                if index[w as usize] == u32::MAX {
                    index[w as usize] = next;
                    low[w as usize] = next;
                    next += 1;
                    stack.push(w);
                    on_stack[w as usize] = true;
                    work.push((w, 0));
                }
                else if on_stack[w as usize] {
                    low[v as usize] = low[v as usize].min(index[w as usize]);
                }
                continue;
            }
            work.pop();
            if let Some(&(parent, _)) = work.last() {
                low[parent as usize] = low[parent as usize].min(low[v as usize]);
            }
            if low[v as usize] == index[v as usize] {
                let mut component = vec![];
                loop {
                    let w = stack.pop().unwrap();
                    on_stack[w as usize] = false;
                    component.push(w);
                    if w == v {
                        break;
                    }
                }
                component.sort_unstable();
                components.push(component);
            }
        }
    }
    let mut result: Vec<Unit> = components
        .into_iter()
        .map(|component| {
            let k = component[0];
            if component.len() == 1 && !edges(k).contains(&k) {
                Unit::Block(k)
            }
            else {
                Unit::Loop { header: k, units: units(&component, successors, Some(k)) }
            }
        })
        .collect();
    result.sort_by_key(Unit::first);
    result
}
