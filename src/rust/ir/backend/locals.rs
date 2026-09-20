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
fn allocate_bounded(region: &Region, mut remaining: usize) -> Result<Allocation, &'static str> {
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
        work = work.saturating_add(live.len().saturating_mul(live.len()));
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
