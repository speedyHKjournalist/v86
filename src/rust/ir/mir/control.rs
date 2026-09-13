//! Dispatcher CFG and typed parallel-copy schedules after local allocation.
use crate::ir::{
    backend::locals::Allocation,
    hir::{self, Region},
    ids::{BlockId, InstId, StateId, ValueId},
    lowering::CompileError,
    types::Type,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Local(usize),
    Scratch(usize),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Copy {
    Save { local: usize, scratch: usize },
    Move { source: Source, destination: usize },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Edge {
    pub target: BlockId,
    pub scratch: Vec<Type>,
    pub copies: Vec<Copy>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Terminator {
    Jump(Edge),
    Branch {
        condition: usize,
        taken: Edge,
        not_taken: Edge,
    },
    Exit(StateId),
}
impl Terminator {
    pub fn edges(&self) -> Vec<&Edge> {
        match self {
            Self::Jump(edge) => vec![edge],
            Self::Branch {
                taken, not_taken, ..
            } => vec![taken, not_taken],
            Self::Exit(_) => vec![],
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Block {
    pub params: Vec<usize>,
    pub instructions: Vec<InstId>,
    pub recovery: Option<StateId>,
    /// Dispatcher work units, distinct from committed guest instructions.
    pub budget_cost: u32,
    pub terminator: Terminator,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ControlFlow {
    pub entries: Vec<BlockId>,
    pub blocks: Vec<Block>,
    /// Every live recovery/observer count has an SSA base, including cycle exits.
    pub dynamic_counts: bool,
    pub polls: Vec<Option<Poll>>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Poll {
    pub recovery: StateId,
    pub cost: u32,
}
fn invalid() -> CompileError {
    CompileError::InvalidIr("invalid lowered control flow".into())
}

/// Resolve simultaneous local assignments. Sinks are safe to write immediately;
/// a cycle saves one source and redirects every use of that old source to scratch.
pub(crate) fn schedule(
    target: BlockId,
    pairs: &[(usize, usize)],
    types: &[Type],
) -> Result<Edge, CompileError> {
    let mut pending = Vec::new();
    let mut uses = vec![0usize; types.len()];
    let mut destinations = std::collections::BTreeSet::new();
    for &(source, destination) in pairs {
        let ty = types.get(source).ok_or_else(invalid)?;
        if Some(ty) != types.get(destination)
            || *ty == Type::Effect
            || !destinations.insert(destination)
        {
            return Err(invalid());
        }
        if source != destination {
            pending.push((Source::Local(source), destination));
            uses[source] += 1;
        }
    }
    let mut edge = Edge {
        target,
        scratch: vec![],
        copies: vec![],
    };
    while !pending.is_empty() {
        if let Some(index) = pending
            .iter()
            .position(|&(_, destination)| uses[destination] == 0)
        {
            let (source, destination) = pending.remove(index);
            if let Source::Local(local) = source {
                uses[local] -= 1;
            }
            edge.copies.push(Copy::Move {
                source,
                destination,
            });
        } else {
            // No sink: every remaining destination supplies another assignment.
            let Source::Local(local) = pending[0].0 else {
                return Err(invalid());
            };
            let scratch = edge.scratch.len();
            edge.scratch.push(types[local]);
            edge.copies.push(Copy::Save { local, scratch });
            uses[local] = 0;
            for (source, _) in &mut pending {
                if *source == Source::Local(local) {
                    *source = Source::Scratch(scratch);
                }
            }
        }
    }
    Ok(edge)
}

pub fn lower(region: &Region, allocation: &Allocation) -> Result<ControlFlow, CompileError> {
    let local = |value: ValueId| -> Result<usize, CompileError> {
        let slot = allocation
            .value_local
            .get(value.index())
            .copied()
            .flatten()
            .ok_or_else(invalid)?;
        if allocation.local_types.get(slot) != region.values.get(value.index()).map(|v| &v.ty) {
            return Err(invalid());
        }
        Ok(slot)
    };
    let edge = |edge: &hir::Edge| -> Result<Edge, CompileError> {
        let params = &region
            .blocks
            .get(edge.target.index())
            .ok_or_else(invalid)?
            .params;
        if params.len() != edge.args.len() {
            return Err(invalid());
        }
        let pairs = edge
            .args
            .iter()
            .zip(params)
            .filter(|(_, param)| region.values[param.index()].ty != Type::Effect)
            .map(|(&arg, &param)| Ok((local(arg)?, local(param)?)))
            .collect::<Result<Vec<_>, CompileError>>()?;
        schedule(edge.target, &pairs, &allocation.local_types)
    };
    let blocks = region
        .blocks
        .iter()
        .map(|block| {
            let terminator = match block.terminator.as_ref().ok_or_else(invalid)? {
                hir::Terminator::Exit(state) => Terminator::Exit(*state),
                hir::Terminator::Branch(next) => Terminator::Jump(edge(next)?),
                hir::Terminator::CondBranch {
                    condition,
                    taken,
                    not_taken,
                } => Terminator::Branch {
                    condition: local(*condition)?,
                    taken: edge(taken)?,
                    not_taken: edge(not_taken)?,
                },
            };
            Ok(Block {
                params: block
                    .params
                    .iter()
                    .filter(|v| region.values[v.index()].ty != Type::Effect)
                    .map(|&v| local(v))
                    .collect::<Result<Vec<_>, _>>()?,
                instructions: block.instructions.clone(),
                recovery: block.entry_state,
                budget_cost: 1,
                terminator,
            })
        })
        .collect::<Result<Vec<_>, CompileError>>()?;
    let mut states = std::collections::BTreeSet::new();
    for block in &region.blocks {
        states.extend(block.entry_state);
        if let Some(hir::Terminator::Exit(state)) = block.terminator {
            states.insert(state);
        }
        for id in &block.instructions {
            let inst = &region.instructions[id.index()];
            states.extend(inst.state);
            states.extend(inst.commit);
        }
    }
    let dynamic_counts = !states.is_empty()
        && states
            .iter()
            .all(|id| region.states[id.index()].count_base.is_some());
    Ok(ControlFlow {
        entries: region.entries.clone(),
        blocks,
        dynamic_counts,
        polls: region
            .instructions
            .iter()
            .map(|inst| {
                if inst.op == hir::Op::PollBudget {
                    Some(Poll {
                        recovery: inst.state.unwrap(),
                        cost: 1,
                    })
                } else {
                    None
                }
            })
            .collect(),
    })
}

pub fn verify(
    region: &Region,
    allocation: &Allocation,
    graph: &ControlFlow,
) -> Result<(), CompileError> {
    if lower(region, allocation)? != *graph {
        return Err(invalid());
    }
    Ok(())
}
impl ControlFlow {
    /// Target restrictions use the lowered graph, not HIR control-flow policy.
    pub fn check_target(&self, cpu: bool) -> Result<(), CompileError> {
        let mut degrees = vec![0usize; self.blocks.len()];
        for block in &self.blocks {
            for edge in block.terminator.edges() {
                *degrees.get_mut(edge.target.index()).ok_or_else(invalid)? += 1;
            }
        }
        for (index, block) in self.blocks.iter().enumerate() {
            if self.entries.contains(&BlockId(index as u32)) {
                if degrees[index] != 0 || !block.params.is_empty() {
                    return Err(CompileError::Unsupported(
                        "entry prologue with internal predecessors/parameters",
                    ));
                }
            } else if block.recovery.is_none() {
                return Err(CompileError::Unsupported(
                    "block budget recovery map missing",
                ));
            }
        }
        if cpu && !self.dynamic_counts {
            // Static recovery counts are valid only for the current acyclic CPU frontend.
            let mut queue: Vec<_> = degrees
                .iter()
                .enumerate()
                .filter(|(_, n)| **n == 0)
                .map(|(b, _)| b)
                .collect();
            let mut visited = 0;
            while let Some(b) = queue.pop() {
                visited += 1;
                for edge in self.blocks[b].terminator.edges() {
                    degrees[edge.target.index()] -= 1;
                    if degrees[edge.target.index()] == 0 {
                        queue.push(edge.target.index());
                    }
                }
            }
            if visited != self.blocks.len() {
                return Err(CompileError::Unsupported(
                    "CPU loop commit accounting pending",
                ));
            }
        }
        Ok(())
    }
}
