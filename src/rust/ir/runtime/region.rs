//! Tier-aware reachable-CFG source selection for automatic IR compilation.
//!
//! This chooses an immutable contiguous byte snapshot; it never performs guest
//! accesses. Reachability uses the shared decoder so Tier 1 and Tier 2 form
//! regions from the same architectural control-flow facts.
use super::{
    compile::{ImmutableCodeSnapshot, Tier},
    entry::CpuEntryKey,
    snapshot::capture,
};
use crate::ir::frontend::decode::{decode, Flow, GuestEip, LinearAddress};
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Policy {
    pub max_bytes: usize,
    pub max_instructions: usize,
}
impl Policy {
    pub fn for_tier(tier: Tier, configured_window: u32) -> Self {
        match tier {
            Tier::One => Self {
                max_bytes: configured_window.clamp(15, 960) as usize,
                max_instructions: 32,
            },
            Tier::Two => Self {
                max_bytes: configured_window.saturating_mul(2).clamp(15, 1920) as usize,
                max_instructions: 96,
            },
        }
    }
}

fn target_offset(
    start: GuestEip,
    instruction: &crate::ir::frontend::decode::DecodedInstruction,
    displacement: i32,
) -> usize {
    let target = instruction.next_pc.0.wrapping_add(displacement as u32);
    let target = if instruction.operand_size == 16 { target & 0xFFFF } else { target };
    target.wrapping_sub(start.0) as usize
}

/// Returns the minimal contiguous prefix containing every reachable decoded
/// instruction that fits the policy. Direct targets outside the captured window
/// remain normal region exits.
pub fn reachable_length(
    bytes: &[u8],
    pc: GuestEip,
    linear: LinearAddress,
    default_32: bool,
    policy: Policy,
) -> usize {
    let limit = bytes.len().min(policy.max_bytes);
    if limit == 0 {
        return 0;
    }
    let bytes = &bytes[..limit];
    let mut pending = BTreeSet::from([0usize]);
    let mut visited = BTreeSet::new();
    let mut max_end = 0usize;
    while let Some(at) = pending.pop_first() {
        if at >= bytes.len() || !visited.insert(at) || visited.len() > policy.max_instructions {
            continue;
        }
        let Ok(instruction) = decode(
            &bytes[at..],
            GuestEip(pc.0.wrapping_add(at as u32)),
            LinearAddress(linear.0.wrapping_add(at as u32)),
            default_32,
        )
        else {
            continue;
        };
        let end = at + instruction.length as usize;
        if end > bytes.len() {
            continue;
        }
        max_end = max_end.max(end);
        match instruction.flow {
            // Legacy analysis uses Boundary for several ordinary basic-block
            // ends (notably memory forms). The IR frontend owns the semantic
            // stop decision, so keep the fallthrough available unless the
            // shared decoder already identifies a baseline #UD form.
            Flow::Next
            | Flow::Boundary
                if !instruction.baseline_ud && !instruction.encoding.block_boundary =>
            {
                if end < bytes.len() {
                    pending.insert(end);
                }
            },
            Flow::Relative {
                displacement,
                conditional,
                call,
            } => {
                if call {
                    continue;
                }
                let target = target_offset(pc, &instruction, displacement);
                if target < bytes.len() {
                    pending.insert(target);
                }
                if conditional && end < bytes.len() {
                    pending.insert(end);
                }
            },
            Flow::Next | Flow::Boundary | Flow::Stop | Flow::Sti => {},
        }
    }
    max_end
}

/// Capture a bounded candidate, then shrink it to the reachable CFG prefix.
/// If a cross-page candidate cannot be observed without side effects, retry the
/// current page so a safe Tier-1 region is still possible.
pub unsafe fn capture_region(
    entry: CpuEntryKey,
    tier: Tier,
    configured_window: u32,
) -> Option<ImmutableCodeSnapshot> {
    let policy = Policy::for_tier(tier, configured_window);
    let page_remaining = 4096 - (entry.linear.0 & 4095) as usize;
    let desired = policy.max_bytes;
    let mut snapshot = match capture(entry.linear.0, desired) {
        Ok(snapshot) => snapshot,
        Err(_) if desired > page_remaining => capture(entry.linear.0, page_remaining).ok()?,
        Err(_) => return None,
    };
    let selected = reachable_length(
        &snapshot.bytes,
        entry.pc,
        entry.linear,
        entry.default_32,
        policy,
    );
    if selected == 0 {
        // Keep a bounded failed-input fingerprint for unchanged-failure
        // suppression, matching the scheduler's previous behavior.
        return Some(snapshot);
    }
    if selected == snapshot.bytes.len() {
        return Some(snapshot);
    }
    snapshot = capture(entry.linear.0, selected).ok()?;
    Some(snapshot)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn follows_forward_edges_loops_and_diamonds() {
        // jmp +2 skips two dead bytes and reaches INC/JMP self.
        let bytes = [0xEB, 0x02, 0xCC, 0xCC, 0x40, 0xEB, 0xFD];
        assert_eq!(
            reachable_length(
                &bytes,
                GuestEip(0x1000),
                LinearAddress(0x2000),
                true,
                Policy {
                    max_bytes: bytes.len(),
                    max_instructions: 32,
                },
            ),
            bytes.len()
        );

        // JZ left; INC; JMP join; DEC; NOP.
        let diamond = [0x74, 0x03, 0x40, 0xEB, 0x01, 0x48, 0x90];
        assert_eq!(
            reachable_length(
                &diamond,
                GuestEip(0x1000),
                LinearAddress(0x2000),
                true,
                Policy {
                    max_bytes: diamond.len(),
                    max_instructions: 32,
                },
            ),
            diamond.len()
        );
    }

    #[test]
    fn boundary_memory_form_keeps_candidate_fallthrough() {
        // MOV EAX,[ESI]; INC EAX. The shared decoder marks the memory form as a
        // legacy block boundary, but the IR CFG frontend can decide whether its
        // precise memory contract permits continuation.
        let bytes = [0x8B, 0x06, 0x40];
        assert_eq!(
            reachable_length(
                &bytes,
                GuestEip(0x1000),
                LinearAddress(0x2000),
                true,
                Policy {
                    max_bytes: bytes.len(),
                    max_instructions: 32,
                },
            ),
            bytes.len()
        );
    }

    #[test]
    fn explicit_semantic_boundary_stops_region_growth() {
        // POP FS is explicitly marked block_boundary by the shared encoding
        // catalogue. Automatic source selection must not speculate past that
        // decoder-owned boundary.
        let bytes = [0x0F, 0xA1, 0x40];
        assert_eq!(
            reachable_length(
                &bytes,
                GuestEip(0x1000),
                LinearAddress(0x2000),
                true,
                Policy {
                    max_bytes: bytes.len(),
                    max_instructions: 32,
                },
            ),
            2
        );
    }

    #[test]
    fn keeps_external_targets_as_region_exits() {
        let bytes = [0xEB, 0x7F, 0x40, 0x40];
        assert_eq!(
            reachable_length(
                &bytes,
                GuestEip(0x1000),
                LinearAddress(0x2000),
                true,
                Policy {
                    max_bytes: bytes.len(),
                    max_instructions: 32,
                },
            ),
            2
        );
    }

    #[test]
    fn tier_policy_is_bounded_and_distinct() {
        let one = Policy::for_tier(Tier::One, 192);
        let two = Policy::for_tier(Tier::Two, 192);
        assert_eq!(one.max_bytes, 192);
        assert_eq!(one.max_instructions, 32);
        assert_eq!(two.max_bytes, 384);
        assert_eq!(two.max_instructions, 96);
        assert!(two.max_bytes > one.max_bytes);
    }
}
