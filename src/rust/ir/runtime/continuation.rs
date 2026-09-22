//! CPU observer continuation contracts. A raw host write need not change an
//! epoch: successful continuation also verifies the active immutable code owner.
use crate::cpu::global_pointers as gp;
use crate::ir::helper::Outcome;

#[derive(PartialEq, Eq)]
pub(super) struct ContinuationContext {
    pub(super) epoch: u64,
    pc: i32,
    previous_pc: i32,
    count: u32,
    controls: [i32; 5],
    mode: [u32; 8],
    segments: [(u16, i32, u32, u8, bool); 8],
    descriptors: [i32; 5],
}
impl ContinuationContext {
    pub(super) unsafe fn capture() -> Self {
        Self {
            epoch: super::live::continuation_epoch(),
            pc: *gp::instruction_pointer,
            previous_pc: *gp::previous_ip,
            count: *gp::instruction_counter,
            controls: std::array::from_fn(|i| *gp::cr.add(i)),
            mode: [
                *gp::protected_mode as u32,
                *gp::is_32 as u32,
                *gp::stack_size_32 as u32,
                *gp::cpl as u32,
                *gp::in_hlt as u32,
                *gp::prefixes as u32,
                (*gp::state_flags).to_u32(),
                (*gp::flags as u32) & !0x8D5, // all non-arithmetic FLAGS affect continuation policy
            ],
            descriptors: [
                *gp::gdtr_offset,
                *gp::gdtr_size,
                *gp::idtr_offset,
                *gp::idtr_size,
                *gp::tss_size_32 as i32,
            ],
            segments: std::array::from_fn(|i| {
                (
                    *gp::sreg.add(i),
                    *gp::segment_offsets.add(i),
                    *gp::segment_limits.add(i),
                    *gp::segment_access_bytes.add(i),
                    *gp::segment_is_null.add(i),
                )
            }),
        }
    }
}

/// Scalar reloads intentionally avoid requiring Wasm SIMD for integer code.
/// XMM backing must survive the observer unchanged before those SSA values may
/// be retained. GPRs and arithmetic FLAGS instead receive new SSA definitions.
struct ScalarSnapshot {
    context: ContinuationContext,
    xmm: [u32; 32],
}
pub(super) struct ScalarObserver(Option<ScalarSnapshot>);
impl ScalarObserver {
    pub(super) unsafe fn capture() -> Self {
        // Declining before the callback is always safe: completion still owns
        // post-state even if the callback clears the pending request. Avoid
        // copying state when we already know this activation will return cold.
        Self(no_pending_irq().then(|| ScalarSnapshot {
            context: ContinuationContext::capture(),
            xmm: std::array::from_fn(|i| *(gp::reg_xmm as *const u32).add(i)),
        }))
    }
    pub(super) unsafe fn finish(self) -> u32 {
        #[cfg(feature = "ir-experimental")]
        let current = self.0.is_some_and(|snapshot| {
            no_pending_irq()
                && snapshot.context.epoch != u64::MAX
                && snapshot.context == ContinuationContext::capture()
                && snapshot.xmm == std::array::from_fn(|i| *(gp::reg_xmm as *const u32).add(i))
                && super::cache::observer_continuation()
        });
        #[cfg(not(feature = "ir-experimental"))]
        let current = {
            let _ = self;
            false
        };
        if current {
            Outcome::Normal as u32
        }
        else {
            // Completion has happened. A failed certificate returns CPU-owned
            // post-state and retires exactly once, never replays the observer.
            *gp::instruction_counter = (*gp::instruction_counter).wrapping_add(1);
            Outcome::Invalidated as u32
        }
    }
}
/// No controller can currently acknowledge a request. Do not consult backed
/// FLAGS here: STI's emitter may still carry IF in SSA. A masked request remains
/// in IRR; an observer which unmasks it is checked again after completion.
pub(super) unsafe fn no_pending_irq() -> bool {
    !*gp::in_hlt
        && !crate::cpu::pic::has_pending_irq()
        && (!*gp::acpi_enabled || !crate::cpu::apic::has_pending_irq())
}
