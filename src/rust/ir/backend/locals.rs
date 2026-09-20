//! Conservative typed interference allocation, including cold-exit StateMap references.
use crate::ir::{hir::*, ids::*, types::Type};
use std::collections::BTreeSet;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Allocation {
    pub value_local: Vec<Option<usize>>,
    pub local_types: Vec<Type>,
}
/// Incremental cliques: pairs already present in the preceding live set are
/// already connected. Dense rows avoid repeated tree insertion of the same edge.
/// This constructs exactly the conservative graph used by both allocators.
pub(crate) struct Interference {
    rows: Vec<Vec<u64>>,
    last: Vec<usize>,
    generation: usize,
    words: usize,
}
impl Interference {
    pub(crate) fn new(values: usize) -> Self {
        Self { rows: vec![vec![]; values], last: vec![0; values], generation: 1,
            words: values.div_ceil(64) }
    }
    /// Charge the incremental algorithm actually executed by connect, rather
    /// than repeatedly charging old edges in otherwise identical live sets.
    pub(crate) fn connection_work(&self, live: &BTreeSet<ValueId>) -> usize {
        let added = live.iter().filter(|v| self.last[v.index()] != self.generation).count();
        live.len().saturating_mul(added + 1)
    }
    fn insert(&mut self, a: usize, b: usize) {
        if self.rows[a].is_empty() { self.rows[a].resize(self.words, 0); }
        self.rows[a][b / 64] |= 1u64 << (b % 64);
    }
    pub(crate) fn connect(&mut self, live: &BTreeSet<ValueId>, ty: impl Fn(ValueId) -> Type) {
        for &a in live {
            if self.last[a.index()] == self.generation { continue; }
            for &b in live {
                if a != b && ty(a) == ty(b) {
                    self.insert(a.index(), b.index());
                    self.insert(b.index(), a.index());
                }
            }
        }
        self.generation += 1;
        for a in live { self.last[a.index()] = self.generation; }
    }
    pub(crate) fn occupied(&self, value: usize, allocation: &Allocation) -> Vec<bool> {
        let mut occupied = vec![false; allocation.local_types.len()];
        for (word, &bits) in self.rows[value].iter().enumerate() {
            let mut bits = bits;
            while bits != 0 {
                let bit = bits.trailing_zeros() as usize;
                if let Some(slot) = allocation.value_local[word * 64 + bit] { occupied[slot] = true; }
                bits &= bits - 1;
            }
        }
        occupied
    }
}
fn state_uses(region: &Region, state: Option<StateId>, live: &mut BTreeSet<ValueId>) {
    if let Some(id) = state {
        live.extend(region.states[id.index()].values());
    }
}
fn term_uses(region: &Region, block: &Block) -> BTreeSet<ValueId> {
    let mut uses = BTreeSet::new();
    match block.terminator.as_ref().unwrap() {
        Terminator::Exit(id) => state_uses(region, Some(*id), &mut uses),
        Terminator::CondBranch { condition, .. } => {
            uses.insert(*condition);
        },
        _ => (),
    }
    for edge in block.terminator.as_ref().unwrap().edges() {
        uses.extend(&edge.args);
    }
    uses
}
pub fn allocate(region: &Region) -> Result<Allocation, &'static str> {
    allocate_bounded(region, 4_000_000)
}
fn allocate_bounded(region: &Region, remaining: usize) -> Result<Allocation, &'static str> {
    if region.blocks.len() == 1 && region.entries == vec![BlockId(0)]
        && matches!(region.blocks[0].terminator, Some(Terminator::Exit(_)))
    {
        return allocate_linear(region, remaining);
    }
    allocate_graph(region, remaining)
}
fn allocate_graph(region: &Region, mut remaining: usize) -> Result<Allocation, &'static str> {
    let mut spend = |amount: usize| -> Result<(), &'static str> {
        remaining = remaining.checked_sub(amount).ok_or("local liveness work budget")?;
        Ok(())
    };
    let n = region.blocks.len();
    let mut inputs = vec![BTreeSet::<ValueId>::new(); n];
    let mut outputs = inputs.clone();
    loop {
        let mut changed = false;
        for b in (0..n).rev() {
            let block = &region.blocks[b];
            let mut live = term_uses(region, block);
            for edge in block.terminator.as_ref().unwrap().edges() {
                spend(inputs[edge.target.index()].len().saturating_mul(region.blocks[edge.target.index()].params.len() + 1))?;
                live.extend(
                    inputs[edge.target.index()]
                        .iter()
                        .filter(|v| !region.blocks[edge.target.index()].params.contains(v)),
                );
            }
            spend(live.len() + 1)?;
            outputs[b] = live.clone();
            for id in block.instructions.iter().rev() {
                let inst = &region.instructions[id.index()];
                for result in &inst.results {
                    live.remove(result);
                }
                live.extend(&inst.args);
                state_uses(region, inst.state, &mut live);
                state_uses(region, inst.commit, &mut live);
                spend(live.len() + inst.results.len() + inst.args.len() + 1)?;
            }
            state_uses(region, block.entry_state, &mut live);
            if live != inputs[b] {
                inputs[b] = live;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    let mut interference = Interference::new(region.values.len());
    let mut work = 0usize;
    let mut connect = |live: &BTreeSet<ValueId>| -> Result<(), &'static str> {
        work = work.saturating_add(interference.connection_work(live));
        if live.len() > 512 || work > 2_000_000 {
            return Err("local allocation work budget");
        }
        interference.connect(live, |v| region.values[v.index()].ty);
        Ok(())
    };
    for (b, block) in region.blocks.iter().enumerate() {
        let mut live = outputs[b].clone();
        connect(&live)?;
        for id in block.instructions.iter().rev() {
            let inst = &region.instructions[id.index()];
            live.extend(&inst.results);
            connect(&live)?;
            for result in &inst.results {
                live.remove(result);
            }
            live.extend(&inst.args);
            state_uses(region, inst.state, &mut live);
            state_uses(region, inst.commit, &mut live);
            connect(&live)?;
        }
        state_uses(region, block.entry_state, &mut live);
        live.extend(&block.params);
        connect(&live)?;
    }
    let mut allocation = Allocation {
        value_local: vec![None; region.values.len()],
        local_types: vec![],
    };
    for (i, value) in region.values.iter().enumerate() {
        if value.ty == Type::Effect {
            continue;
        }
        let occupied = interference.occupied(i, &allocation);
        let slot = allocation
            .local_types
            .iter()
            .enumerate()
            .find(|(i, ty)| **ty == value.ty && !occupied[*i])
            .map(|(i, _)| i)
            .unwrap_or_else(|| {
                let i = allocation.local_types.len();
                allocation.local_types.push(value.ty);
                i
            });
        allocation.value_local[i] = Some(slot);
    }
    Ok(allocation)
}

/// Straight-line SSA has exact definition/last-use intervals. Include every
/// before/after recovery operand, not merely normal dataflow. Two positions per
/// instruction allow normal results to reuse dying pre-call snapshot slots only
/// after the emitter has staged returns and checked the fault outcome.
/// The independent owned-MIR verifier still checks all live-set interference.
fn allocate_linear(region: &Region, mut remaining: usize) -> Result<Allocation, &'static str> {
    use std::{cmp::Reverse, collections::{BinaryHeap, HashMap}};
    let mut spend = |n: usize| -> Result<(), &'static str> {
        remaining = remaining.checked_sub(n).ok_or("local liveness work budget")?;
        Ok(())
    };
    let block = &region.blocks[0];
    spend(region.values.len() + block.instructions.len() + 1)?;
    let mut first = vec![None; region.values.len()];
    let mut last = vec![0; region.values.len()];
    for &value in &block.params { first[value.index()] = Some(0); }
    for (position, id) in block.instructions.iter().enumerate() {
        let inst = &region.instructions[id.index()];
        // A committing access's after-state can reference its own results while
        // the before-state and address remain live. Keep that overlap intact.
        let definition = position * 2 + if inst.commit.is_some() { 1 } else { 2 };
        for value in &inst.results {
            first[value.index()] = Some(definition);
            last[value.index()] = definition;
        }
    }
    let mut observe = |value: ValueId, position: usize| -> Result<(), &'static str> {
        spend(1)?;
        if region.values[value.index()].ty == Type::Effect { return Ok(()); }
        if first[value.index()].is_none_or(|start| start > position) {
            return Err("invalid linear allocation use");
        }
        last[value.index()] = last[value.index()].max(position);
        Ok(())
    };
    if let Some(state) = block.entry_state {
        for value in region.states[state.index()].values() { observe(value, 0)?; }
    }
    for (position, id) in block.instructions.iter().enumerate() {
        let inst = &region.instructions[id.index()];
        for &value in &inst.args { observe(value, position * 2 + 1)?; }
        for state in inst.state.into_iter().chain(inst.commit) {
            for value in region.states[state.index()].values() { observe(value, position * 2 + 1)?; }
        }
    }
    let Terminator::Exit(state) = block.terminator.as_ref().unwrap() else { unreachable!() };
    for value in region.states[state.index()].values() { observe(value, block.instructions.len() * 2 + 1)?; }
    let mut intervals: Vec<_> = first.iter().enumerate().filter_map(|(value, &start)|
        start.filter(|_| region.values[value].ty != Type::Effect)
            .map(|start| (start, last[value], value))).collect();
    intervals.sort_unstable();
    let mut allocation = Allocation { value_local: vec![None; region.values.len()], local_types: vec![] };
    let mut active = BinaryHeap::<Reverse<(usize, usize)>>::new();
    let mut free = HashMap::<Type, Vec<usize>>::new();
    for (start, end, value) in intervals {
        while active.peek().is_some_and(|Reverse((end, _))| *end < start) {
            let Reverse((_, slot)) = active.pop().unwrap();
            free.entry(allocation.local_types[slot]).or_default().push(slot);
        }
        if active.len() >= 512 { return Err("local allocation work budget"); }
        let ty = region.values[value].ty;
        let slot = free.get_mut(&ty).and_then(Vec::pop).unwrap_or_else(|| {
            let slot = allocation.local_types.len(); allocation.local_types.push(ty); slot
        });
        allocation.value_local[value] = Some(slot);
        active.push(Reverse((end, slot)));
    }
    // Removed arena instructions are never executed but still have typed plans.
    // Give their dead values a legal local without extending any live interval.
    for (value, definition) in region.values.iter().enumerate() {
        if definition.ty != Type::Effect && allocation.value_local[value].is_none() {
            let slot = allocation.local_types.iter().position(|&ty| ty == definition.ty).unwrap_or_else(|| {
                let slot = allocation.local_types.len(); allocation.local_types.push(definition.ty); slot
            });
            allocation.value_local[value] = Some(slot);
        }
    }
    Ok(allocation)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn liveness_has_a_bounded_failure_path() {
        use crate::ir::frontend::{decode::{GuestEip, LinearAddress}, lift::lift_cpu};
        let region = lift_cpu(&[0x40, 0x43], GuestEip(0), LinearAddress(0), true).unwrap();
        assert_eq!(allocate_bounded(&region, 1).unwrap_err(), "local liveness work budget");
        assert!(allocate(&region).is_ok());
    }
    #[test]
    fn linear_intervals_preserve_cold_uses_and_typed_results() {
        use crate::ir::frontend::{decode::{GuestEip, LinearAddress}, lift::lift_cpu};
        for bytes in [vec![0x40; 96], vec![0x8B,0x06,0x40,0x89,0x06],
            vec![0xF0,0x01,0x06], vec![0x0F,0x31,0x40], vec![0xEC,0x43],
            vec![0x66,0x0F,0xEF,0xC0,0x40]] {
            let mut region = lift_cpu(&bytes, GuestEip(0x1000), LinearAddress(0x1000), true).unwrap();
            for optimized in [false, true] {
                if optimized { crate::ir::passes::run(&mut region, crate::ir::passes::PassConfig::tier1()).unwrap(); }
                let fast = allocate_linear(&region, 4_000_000).unwrap();
                let reference = allocate_graph(&region, 4_000_000).unwrap();
                assert!(fast.local_types.len() <= reference.local_types.len() + 16);
                // Independently verifies dominance, hidden recovery uses,
                // simultaneous live locals and helper normal-result ownership.
                crate::ir::lowering::lower(&region).unwrap().verify().unwrap();
            }
        }
    }
    /// Opt-in allocator-only microbenchmark; never a host-dependent CI gate.
    #[test]
    #[ignore]
    fn linear_allocation_paired_benchmark() {
        use crate::ir::frontend::{decode::{GuestEip, LinearAddress}, lift::lift_cpu};
        for size in [16, 32, 96] {
            let region = lift_cpu(&vec![0x40; size], GuestEip(0), LinearAddress(0), true).unwrap();
            let mut timings = [Vec::new(), Vec::new()];
            for round in 0..7 {
                for index in [round % 2, 1 - round % 2] {
                    let start = std::time::Instant::now();
                    for _ in 0..40 {
                        let allocation = if index == 0 { allocate_graph(&region, 4_000_000) }
                            else { allocate_linear(&region, 4_000_000) }.unwrap();
                        std::hint::black_box(allocation);
                    }
                    timings[index].push(start.elapsed().as_secs_f64() * 1e6 / 40.0);
                }
            }
            for times in &mut timings { times.sort_by(f64::total_cmp); }
            println!("{size} instructions: graph {:.1} us, intervals {:.1} us (paired median, allocation only)",
                timings[0][3], timings[1][3]);
        }
    }
    #[test]
    fn incremental_interference_matches_full_cliques() {
        let n = 137;
        let mut graph = Interference::new(n);
        let mut reference = vec![BTreeSet::new(); n];
        let types: Vec<_> = (0..n).map(|i| if i % 3 == 0 { Type::I64 } else { Type::I32 }).collect();
        let mut rng = 17u32;
        let mut live = BTreeSet::new();
        for step in 0..2000 {
            rng ^= rng << 13; rng ^= rng >> 17; rng ^= rng << 5;
            let value = ValueId(rng % n as u32);
            if step % 37 == 0 { live.clear(); }
            if step % 3 == 0 { live.remove(&value); } else { live.insert(value); }
            graph.connect(&live, |v| types[v.index()]);
            for &a in &live { for &b in &live {
                if a != b && types[a.index()] == types[b.index()] { reference[a.index()].insert(b); }
            } }
        }
        let allocation = Allocation { value_local: (0..n).map(Some).collect(), local_types: types };
        for i in 0..n {
            let actual = graph.occupied(i, &allocation);
            for j in 0..n { assert_eq!(actual[j], reference[i].contains(&ValueId(j as u32)), "{i}/{j}"); }
        }
    }
}
