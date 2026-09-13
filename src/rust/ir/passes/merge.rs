//! Eliminate straight-line dispatcher edges while preserving every budget boundary.
use super::{replace, PassStats};
use crate::ir::{hir::*, ids::*, types::Type};

pub(super) fn run(region: &mut Region, stats: &mut PassStats) -> Result<(), String> {
    if region.blocks.len() > 64 {
        return Err("CFG merge block budget exceeded".into());
    }
    // Each iteration removes one block; no graph growth or convergence heuristic.
    for _ in 0..64 {
        let mut predecessors = vec![0; region.blocks.len()];
        for block in &region.blocks {
            for edge in block.terminator.as_ref().unwrap().edges() {
                predecessors[edge.target.index()] += 1;
            }
        }
        let candidate = region.blocks.iter().enumerate().find_map(|(a, block)| {
            let Terminator::Branch(edge) = block.terminator.as_ref()? else {
                return None;
            };
            let b = edge.target.index();
            if a == b || predecessors[b] != 1 || region.entries.contains(&edge.target) {
                return None;
            }
            let target = &region.blocks[b];
            let recovery = target.entry_state?;
            let effect_index = target
                .params
                .iter()
                .position(|v| region.values[v.index()].ty == Type::Effect)?;
            Some((a, b, edge.args.clone(), effect_index, recovery))
        });
        let Some((a, b, args, effect_index, recovery)) = candidate else {
            break;
        };
        if region.instructions.len() >= 8192 || region.values.len() >= 16384 {
            break;
        }
        let params = region.blocks[b].params.clone();
        region.blocks[a].terminator = None;
        let effect = region.append(
            BlockId(a as u32),
            Op::PollBudget,
            vec![args[effect_index]],
            &[Type::Effect],
            Some(recovery),
        )[0];
        // Inputs cannot depend on the target's parameters: its only predecessor
        // is this distinct block, and the verified graph has no entry at target.
        for (p, (&param, &arg)) in params.iter().zip(&args).enumerate() {
            replace(region, param, if p == effect_index { effect } else { arg });
        }
        let instructions = std::mem::take(&mut region.blocks[b].instructions);
        region.blocks[a].instructions.extend(instructions);
        region.blocks[a].terminator = region.blocks[b].terminator.take();
        region.blocks.remove(b);
        let remap = |id: &mut BlockId| {
            if id.index() == b {
                *id = BlockId(a as u32);
            }
            if id.index() > b {
                id.0 -= 1;
            }
        };
        for entry in &mut region.entries {
            remap(entry);
        }
        for inst in &mut region.instructions {
            remap(&mut inst.block);
        }
        for value in &mut region.values {
            if let Definition::Parameter(ref mut block, _) = value.definition {
                remap(block);
            }
        }
        for block in &mut region.blocks {
            for edge in block.terminator.as_mut().unwrap().edges_mut() {
                remap(&mut edge.target);
            }
        }
        stats.merged += 1;
    }
    Ok(())
}
