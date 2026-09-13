//! Conservative typed interference allocation, including cold-exit StateMap references.
use crate::ir::{hir::*, ids::*, types::Type};
use std::collections::BTreeSet;
#[derive(Debug, PartialEq, Eq)]
pub struct Allocation {
    pub value_local: Vec<Option<usize>>,
    pub local_types: Vec<Type>,
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
    let n = region.blocks.len();
    let mut inputs = vec![BTreeSet::<ValueId>::new(); n];
    let mut outputs = inputs.clone();
    loop {
        let mut changed = false;
        for b in (0..n).rev() {
            let block = &region.blocks[b];
            let mut live = term_uses(region, block);
            for edge in block.terminator.as_ref().unwrap().edges() {
                live.extend(
                    inputs[edge.target.index()]
                        .iter()
                        .filter(|v| !region.blocks[edge.target.index()].params.contains(v)),
                );
            }
            outputs[b] = live.clone();
            for id in block.instructions.iter().rev() {
                let inst = &region.instructions[id.index()];
                for result in &inst.results {
                    live.remove(result);
                }
                live.extend(&inst.args);
                state_uses(region, inst.state, &mut live);
                state_uses(region, inst.commit, &mut live);
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
    let mut interference = vec![BTreeSet::new(); region.values.len()];
    let mut work = 0usize;
    let mut connect = |live: &BTreeSet<ValueId>| -> Result<(), &'static str> {
        work = work.saturating_add(live.len().saturating_mul(live.len()));
        if live.len() > 512 || work > 2_000_000 {
            return Err("local allocation work budget");
        }
        for &a in live {
            for &b in live {
                if a != b && region.values[a.index()].ty == region.values[b.index()].ty {
                    interference[a.index()].insert(b);
                }
            }
        }
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
        let occupied: BTreeSet<_> = interference[i]
            .iter()
            .filter_map(|v| allocation.value_local[v.index()])
            .collect();
        let slot = allocation
            .local_types
            .iter()
            .enumerate()
            .find(|(i, ty)| **ty == value.ty && !occupied.contains(i))
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
