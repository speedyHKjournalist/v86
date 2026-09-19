//! Owned use/definition facts for allocation after machine rewrites.
//! Recovery uses are conservative for both CPU and standalone ABIs.
use super::{control, value::Step, MirData};
use crate::ir::{backend::locals::Allocation, hir, ids::*, lowering::CompileError, types::Type};
use std::collections::BTreeSet;
#[derive(Clone, Debug, PartialEq, Eq)]
struct Instruction {
    uses: Vec<ValueId>,
    recovery: Vec<ValueId>,
    definitions: Vec<ValueId>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct Edge {
    target: BlockId,
    arguments: Vec<ValueId>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
enum Terminator {
    Exit(StateId),
    Jump(Edge),
    Branch(ValueId, Edge, Edge),
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct Block {
    params: Vec<ValueId>,
    instructions: Vec<InstId>,
    recovery: Vec<ValueId>,
    exit_uses: Vec<ValueId>,
    terminator: Terminator,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Graph {
    instructions: Vec<Instruction>,
    blocks: Vec<Block>,
}
fn state(region: &hir::Region, id: Option<StateId>) -> Vec<ValueId> {
    id.map(|s| region.states[s.index()].values())
        .unwrap_or_default()
}
pub fn capture(region: &hir::Region) -> Graph {
    let edge = |e: &hir::Edge| Edge {
        target: e.target,
        arguments: e.args.clone(),
    };
    Graph {
        instructions: region
            .instructions
            .iter()
            .map(|i| Instruction {
                uses: i.args.clone(),
                recovery: state(region, i.state)
                    .into_iter()
                    .chain(state(region, i.commit))
                    .collect(),
                definitions: i.results.clone(),
            })
            .collect(),
        blocks: region
            .blocks
            .iter()
            .map(|b| {
                let t = b.terminator.as_ref().unwrap();
                Block {
                    params: b.params.clone(),
                    instructions: b.instructions.clone(),
                    recovery: state(region, b.entry_state),
                    exit_uses: if let hir::Terminator::Exit(s) = t {
                        state(region, Some(*s))
                    } else {
                        vec![]
                    },
                    terminator: match t {
                        hir::Terminator::Exit(s) => Terminator::Exit(*s),
                        hir::Terminator::Branch(e) => Terminator::Jump(edge(e)),
                        hir::Terminator::CondBranch {
                            condition,
                            taken,
                            not_taken,
                        } => Terminator::Branch(*condition, edge(taken), edge(not_taken)),
                    },
                }
            })
            .collect(),
    }
}
fn spend(left: &mut usize, n: usize) -> Result<(), CompileError> {
    *left = left
        .checked_sub(n)
        .ok_or(CompileError::Budget("machine allocation/scheduling work"))?;
    Ok(())
}
pub(super) fn expression(steps: &[Step]) -> Vec<ValueId> {
    steps
        .iter()
        .flat_map(|s| match s {
            Step::Value(v) => vec![*v],
            Step::Packed {
                destination,
                source,
                ..
            } => vec![*destination, *source],
            _ => vec![],
        })
        .collect()
}
fn uses(data: &MirData, id: InstId) -> Vec<ValueId> {
    if data.stack_elided[id.index()] {
        return vec![];
    }
    let i = &data.allocation_graph.instructions[id.index()];
    let mut out =
        if let Some(p) = &data.values[id.index()] { expression(&p.steps) } else { i.uses.clone() };
    out.extend(&i.recovery);
    out
}
fn edges(t: &Terminator) -> Vec<&Edge> {
    match t {
        Terminator::Exit(_) => vec![],
        Terminator::Jump(e) => vec![e],
        Terminator::Branch(_, a, b) => vec![a, b],
    }
}
fn term_uses(b: &Block) -> Vec<ValueId> {
    let mut out = b.exit_uses.clone();
    if let Terminator::Branch(v, _, _) = b.terminator {
        out.push(v);
    }
    for edge in edges(&b.terminator) {
        out.extend(&edge.arguments);
    }
    out
}
pub(super) fn use_counts(data: &MirData, limit: &mut usize) -> Result<Vec<usize>, CompileError> {
    let mut counts = vec![0; data.value_types.len()];
    for b in &data.allocation_graph.blocks {
        let mut values = term_uses(b);
        values.extend(&b.recovery);
        for &i in &b.instructions {
            values.extend(uses(data, i));
        }
        spend(limit, values.len() + 1)?;
        for v in values {
            counts[v.index()] += 1;
        }
    }
    Ok(counts)
}
pub(super) fn blocks(data: &MirData) -> Vec<Vec<InstId>> {
    data.allocation_graph
        .blocks
        .iter()
        .map(|b| b.instructions.clone())
        .collect()
}
pub fn reallocate(data: &mut MirData, work_limit: usize) -> Result<usize, CompileError> {
    let graph = &data.allocation_graph;
    let mut left = work_limit;
    spend(
        &mut left,
        data.value_types.len() + graph.instructions.len() + graph.blocks.len(),
    )?;
    let mut inputs = vec![BTreeSet::new(); graph.blocks.len()];
    let mut outputs = inputs.clone();
    let dependencies: Vec<_> = (0..graph.instructions.len())
        .map(|n| uses(data, InstId(n as u32)))
        .collect();
    loop {
        let mut changed = false;
        for (n, b) in graph.blocks.iter().enumerate().rev() {
            let mut live: BTreeSet<_> = term_uses(b).into_iter().collect();
            for edge in edges(&b.terminator) {
                let target = &graph.blocks[edge.target.index()];
                live.extend(
                    inputs[edge.target.index()]
                        .iter()
                        .filter(|v| !target.params.contains(v)),
                );
            }
            spend(&mut left, live.len() + 1)?;
            outputs[n] = live.clone();
            for &id in b.instructions.iter().rev() {
                if data.stack_elided[id.index()] {
                    continue;
                }
                for v in &graph.instructions[id.index()].definitions {
                    live.remove(v);
                }
                live.extend(&dependencies[id.index()]);
                spend(&mut left, live.len() + 1)?;
            }
            live.extend(&b.recovery);
            if inputs[n] != live {
                inputs[n] = live;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    let mut interference = vec![BTreeSet::new(); data.value_types.len()];
    let mut connect = |live: &BTreeSet<ValueId>| -> Result<(), CompileError> {
        spend(&mut left, live.len().saturating_mul(live.len()))?;
        for &a in live {
            for &b in live {
                if a != b && data.value_types[a.index()] == data.value_types[b.index()] {
                    interference[a.index()].insert(b);
                }
            }
        }
        Ok(())
    };
    let mut active: BTreeSet<ValueId> = BTreeSet::new();
    for (n, b) in graph.blocks.iter().enumerate() {
        let mut live = outputs[n].clone();
        connect(&live)?;
        active.extend(&live);
        for &id in b.instructions.iter().rev() {
            if data.stack_elided[id.index()] {
                continue;
            }
            let defs = &graph.instructions[id.index()].definitions;
            live.extend(defs);
            active.extend(defs);
            connect(&live)?;
            for v in defs {
                live.remove(v);
            }
            live.extend(&dependencies[id.index()]);
            active.extend(&live);
            connect(&live)?;
        }
        live.extend(&b.recovery);
        live.extend(&b.params);
        active.extend(&live);
        connect(&live)?;
    }
    let mut allocation = Allocation {
        value_local: vec![None; data.value_types.len()],
        local_types: vec![],
    };
    for v in active {
        let ty = data.value_types[v.index()];
        if ty == Type::Effect {
            continue;
        }
        let occupied: BTreeSet<_> = interference[v.index()]
            .iter()
            .filter_map(|v| allocation.value_local[v.index()])
            .collect();
        let slot = allocation
            .local_types
            .iter()
            .enumerate()
            .find(|(n, t)| **t == ty && !occupied.contains(n))
            .map(|(n, _)| n)
            .unwrap_or_else(|| {
                let n = allocation.local_types.len();
                allocation.local_types.push(ty);
                n
            });
        allocation.value_local[v.index()] = Some(slot);
    }
    let local = |v: ValueId| {
        allocation.value_local[v.index()]
            .ok_or_else(|| CompileError::InvalidIr("missing machine local".into()))
    };
    let edge = |e: &Edge| -> Result<control::Edge, CompileError> {
        let pairs = e
            .arguments
            .iter()
            .zip(&graph.blocks[e.target.index()].params)
            .filter(|(_, v)| data.value_types[v.index()] != Type::Effect)
            .map(|(&a, &b)| Ok((local(a)?, local(b)?)))
            .collect::<Result<Vec<_>, CompileError>>()?;
        control::schedule(e.target, &pairs, &allocation.local_types)
    };
    let mut control = data.control.clone();
    for (b, source) in control.blocks.iter_mut().zip(&graph.blocks) {
        b.params = source
            .params
            .iter()
            .filter(|v| data.value_types[v.index()] != Type::Effect)
            .map(|&v| local(v))
            .collect::<Result<_, _>>()?;
        b.terminator = match &source.terminator {
            Terminator::Exit(s) => control::Terminator::Exit(*s),
            Terminator::Jump(e) => control::Terminator::Jump(edge(e)?),
            Terminator::Branch(v, a, b) => control::Terminator::Branch {
                condition: local(*v)?,
                taken: edge(a)?,
                not_taken: edge(b)?,
            },
        };
    }
    let saved = data
        .allocation
        .local_types
        .len()
        .saturating_sub(allocation.local_types.len());
    data.allocation = allocation;
    data.control = control;
    Ok(saved)
}
