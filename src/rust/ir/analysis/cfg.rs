use crate::ir::{hir::Region, ids::BlockId};
#[derive(Debug)]
pub struct Cfg {
    pub predecessors: Vec<Vec<BlockId>>,
    pub reachable: Vec<bool>,
    pub dominates: Vec<Vec<bool>>,
}
impl Cfg {
    pub fn compute(region: &Region) -> Result<Self, &'static str> {
        let n = region.blocks.len();
        let mut predecessors = vec![vec![]; n];
        let mut reachable = vec![false; n];
        for (i, block) in region.blocks.iter().enumerate() {
            let term = block.terminator.as_ref().ok_or("unterminated block")?;
            for edge in term.edges() {
                let pred = predecessors
                    .get_mut(edge.target.index())
                    .ok_or("invalid edge target")?;
                pred.push(BlockId(i as u32));
            }
        }
        if region.entries.is_empty() {
            return Err("region has no entry");
        }
        let mut queue = region.entries.clone();
        while let Some(block) = queue.pop() {
            let seen = reachable.get_mut(block.index()).ok_or("invalid entry")?;
            if *seen {
                continue;
            }
            *seen = true;
            queue.extend(
                region.blocks[block.index()]
                    .terminator
                    .as_ref()
                    .unwrap()
                    .edges()
                    .iter()
                    .map(|e| e.target),
            );
        }
        // Synthetic root reaches every entry, even when it also has a backedge.
        let mut dominates = vec![vec![true; n]; n];
        for i in 0..n {
            if !reachable[i] || region.entries.contains(&BlockId(i as u32)) {
                dominates[i].fill(false);
                dominates[i][i] = true;
            }
        }
        loop {
            let mut changed = false;
            for i in 0..n {
                if !reachable[i] || region.entries.contains(&BlockId(i as u32)) {
                    continue;
                }
                let mut next = vec![true; n];
                for pred in predecessors[i].iter().filter(|p| reachable[p.index()]) {
                    for j in 0..n {
                        next[j] &= dominates[pred.index()][j];
                    }
                }
                next[i] = true;
                if next != dominates[i] {
                    dominates[i] = next;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
        Ok(Self {
            predecessors,
            reachable,
            dominates,
        })
    }
}
