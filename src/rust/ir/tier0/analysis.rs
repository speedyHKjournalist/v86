//! Basic blocks of one guest code page, discovered from its observed entries.
//!
//! Blocks are straight-line runs of complete instructions inside the page.
//! A block ends after a control transfer, a block-boundary encoding (mode,
//! paging and I/O instructions) or an instruction without a template (both
//! run in the interpreter), before another block's start, or before an
//! instruction that cannot be decoded within the page. Each instruction
//! belongs to one block, except where a branch enters the middle of another
//! instruction (a different decoding of the same bytes).
use crate::ir::frontend::decode::{decode, DecodedInstruction, Flow, GuestEip, LinearAddress};
use std::collections::BTreeSet;

pub const PAGE: usize = 4096;
/// Bounded work per page, and a function size V8's optimizing tier handles
/// comfortably (the legacy JIT caps a module at about 250 extra blocks).
const MAX_BLOCKS: usize = 512;
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
    /// The part of `external` that continues the code rather than calling
    /// other code: jump targets, and falling (or an instruction running)
    /// off the end of the analyzed bytes.
    pub jumps: Vec<i64>,
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
    let decode_at = |at: usize| {
        decode(
            &bytes[at..],
            GuestEip(base_pc.0.wrapping_add(at as u32)),
            LinearAddress(base_linear.0.wrapping_add(at as u32)),
            default_32,
        )
        .ok()
    };
    // Discovery: every run from a block start, until a block-ending
    // instruction or an instruction another run already decoded (whose
    // start becomes a block start: runs share no instructions).
    let mut leader = vec![false; size];
    let mut decoded_at = vec![false; size];
    let mut external = vec![];
    let mut jumps = vec![];
    let mut pending: BTreeSet<usize> = entries.iter().copied().filter(|&at| at < size).collect();
    let mut total = 0;
    let mut leaders = 0;
    let next = |pending: &mut BTreeSet<usize>| {
        let preferred = pending.range(prefer.clone()).next().copied();
        preferred.map(|at| pending.take(&at).unwrap()).or_else(|| pending.pop_first())
    };
    while let Some(start) = next(&mut pending) {
        if leader[start] || leaders == MAX_BLOCKS || total >= MAX_INSTRUCTIONS {
            continue;
        }
        if decoded_at[start] {
            leader[start] = true;
            leaders += 1;
            continue;
        }
        let mut at = start;
        while at < size && !decoded_at[at] && total < MAX_INSTRUCTIONS {
            // Undecodable or crossing the page end: left to the interpreter.
            let Some(decoded) = decode_at(at)
            else {
                if size - at < 15 {
                    jumps.push(size as i64);
                }
                break;
            };
            if at == start {
                leader[start] = true;
                leaders += 1;
            }
            decoded_at[at] = true;
            total += 1;
            let length = decoded.length as usize;
            if ends_block(&decoded) {
                let call = matches!(decoded.flow, Flow::Relative { call: true, .. });
                for (k, target) in successors(&decoded, at, base_pc).into_iter().enumerate() {
                    if (0..size as i64).contains(&target) {
                        pending.insert(target as usize);
                    }
                    else {
                        external.push(target);
                        if k > 0 || !call {
                            jumps.push(target);
                        }
                    }
                }
                // Interpreted instructions continue at their fall-through.
                if matches!(decoded.flow, Flow::Next | Flow::Boundary) {
                    if at + length < size {
                        pending.insert(at + length);
                    }
                    else {
                        external.push((at + length) as i64);
                        jumps.push((at + length) as i64);
                    }
                }
                break;
            }
            at += length;
            if at >= size {
                external.push(at as i64);
                jumps.push(at as i64);
            }
            else if decoded_at[at] {
                pending.insert(at);
            }
        }
    }
    // Blocks: each run split at the block starts inside it, in address order
    // (a jump to a higher address is a forward branch in the emitted
    // function, see emit::Page::goto_linear).
    let mut blocks: Vec<Block> = vec![];
    let mut block_at = vec![None; size];
    for start in (0..size).filter(|&at| leader[at]) {
        let mut instructions = vec![];
        let mut at = start;
        while let Some(decoded) = decode_at(at) {
            let length = decoded.length as usize;
            let last = ends_block(&decoded);
            instructions.push(Instruction { offset: at as u16, decoded });
            at += length;
            // A run cut by the instruction budget continues by dispatch.
            if last || at >= size || leader[at] || !decoded_at[at] {
                break;
            }
        }
        block_at[start] = Some(blocks.len() as u32);
        blocks.push(Block { start: start as u16, instructions });
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
    PagePlan { blocks, block_at, return_site, external, jumps }
}

/// Which of `entries` (offsets, in priority order) a recompilation must seed:
/// those no earlier seed reaches through boundaries every analysis finds
/// (the targets and fall-throughs of block-ending instructions). An entry
/// that is a block start only because it was seeded itself, such as a point
/// inside a straight-line run, stays a seed.
pub fn seeds(plan: &PagePlan, entries: &[usize]) -> Vec<bool> {
    let size = plan.block_at.len();
    let mut reached = vec![false; plan.blocks.len()];
    let mut work = vec![];
    entries
        .iter()
        .map(|&at| {
            let Some(Some(k)) = plan.block_at.get(at).copied()
            else {
                return false;
            };
            if reached[k as usize] {
                return false;
            }
            work.push(k);
            reached[k as usize] = true;
            while let Some(k) = work.pop() {
                let b = &plan.blocks[k as usize];
                let last = b.instructions.last().unwrap();
                if !ends_block(&last.decoded) {
                    continue;
                }
                let base = GuestEip(last.decoded.instruction_pc.0.wrapping_sub(last.offset as u32));
                let mut targets = successors(&last.decoded, last.offset as usize, base);
                if matches!(last.decoded.flow, Flow::Next | Flow::Boundary) {
                    targets.push(b.end() as i64);
                }
                for t in targets {
                    if let Some(Some(next)) = usize::try_from(t).ok().filter(|&t| t < size).map(|t| plan.block_at[t]) {
                        if !reached[next as usize] {
                            reached[next as usize] = true;
                            work.push(next);
                        }
                    }
                }
            }
            true
        })
        .collect()
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
    let mut layout = units(&all, &successors, None);
    // Each loop dispatches through a table over its members' offsets (see
    // emit::emit_units); V8 compiles functions with very large tables
    // slowly or not at all (its per-function zone limit). Loops over too
    // wide a range, then the widest ones, become plain blocks of the
    // enclosing level (their back edges then dispatch there).
    let start = |k: u32| plan.blocks[k as usize].start as usize;
    let range = |unit: &Unit| {
        let mut blocks = vec![];
        unit.blocks(&mut blocks);
        blocks.iter().map(|&k| start(k)).max().unwrap() - start(unit.first()) + 1
    };
    flatten(&mut layout, &|unit| range(unit) > MAX_LOOP_TABLE);
    loop {
        let mut total = 0;
        let mut widest = 0;
        visit_loops(&layout, &mut |unit| {
            total += range(unit);
            widest = widest.max(range(unit));
        });
        if total <= MAX_LOOP_TABLES {
            break;
        }
        flatten(&mut layout, &|unit| range(unit) == widest);
    }
    layout
}
/// Loop dispatch table bounds: per loop, and for all loops of a function.
const MAX_LOOP_TABLE: usize = 1024;
const MAX_LOOP_TABLES: usize = 8192;
fn visit_loops(units: &[Unit], f: &mut dyn FnMut(&Unit)) {
    for unit in units {
        if let Unit::Loop { units, .. } = unit {
            f(unit);
            visit_loops(units, f);
        }
    }
}
/// Replace the loops matching `wide` by their units (in address order).
fn flatten(units: &mut Vec<Unit>, wide: &dyn Fn(&Unit) -> bool) {
    let mut out = Vec::with_capacity(units.len());
    for unit in std::mem::take(units) {
        let flat = matches!(unit, Unit::Loop { .. }) && wide(&unit);
        match unit {
            Unit::Loop { header, units: mut inner } => {
                flatten(&mut inner, wide);
                if flat {
                    out.extend(inner);
                }
                else {
                    out.push(Unit::Loop { header, units: inner });
                }
            },
            unit => out.push(unit),
        }
    }
    out.sort_by_key(Unit::first);
    *units = out;
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
