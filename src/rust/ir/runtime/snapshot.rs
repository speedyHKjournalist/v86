//! Read-only code capture. This never calls the CPU's MMIO-capable page walker.
use super::compile::{CodeDependency, CodeMapping, ImmutableCodeSnapshot};
use crate::cpu::{cpu, global_pointers as gp, memory};
use crate::ir::frontend::decode::{LinearAddress, PhysicalAddress};
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaptureError {
    Size,
    Unreadable,
    NonRam,
    UnsupportedPaging,
}
unsafe fn ram(address: u32, bytes: usize) -> Result<&'static [u8], CaptureError> {
    let last = address
        .checked_add(bytes as u32 - 1)
        .ok_or(CaptureError::NonRam)?;
    if memory::mem8.is_null() || memory::in_mapped_range(address) || memory::in_mapped_range(last) {
        return Err(CaptureError::NonRam);
    }
    Ok(std::slice::from_raw_parts(
        memory::mem8.add(address as usize),
        bytes,
    ))
}
unsafe fn table(address: u32, pae: bool) -> Result<u32, CaptureError> {
    let bytes = ram(address, if pae { 8 } else { 4 })?;
    if pae && bytes[4..].iter().any(|b| *b != 0) {
        return Err(CaptureError::UnsupportedPaging);
    }
    Ok(u32::from_le_bytes(bytes[..4].try_into().unwrap()))
}
pub unsafe fn translate(linear: u32) -> Result<u32, CaptureError> {
    let user = *gp::cpl == 3;
    let cached = cpu::tlb_data[(linear >> 12) as usize];
    if cached & (cpu::TLB_VALID | if user { cpu::TLB_NO_USER } else { 0 }) == cpu::TLB_VALID {
        return Ok(((cached as u32 & !4095) ^ linear).wrapping_sub(memory::mem8 as u32));
    }
    let cr0 = *gp::cr;
    if cr0 & cpu::CR0_PG == 0 {
        return Ok(linear);
    }
    let cr4 = *gp::cr.add(4);
    let pae = cr4 & cpu::CR4_PAE != 0;
    let directory = if pae {
        let pdpte = *gp::reg_pdpte.add((linear >> 30) as usize);
        if pdpte >> 32 != 0 {
            return Err(CaptureError::UnsupportedPaging);
        }
        if pdpte & 1 == 0 {
            return Err(CaptureError::Unreadable);
        }
        (pdpte as u32 & !4095).wrapping_add((linear >> 21 & 511) * 8)
    } else {
        // Match the baseline's CR3 low-bit policy, including its unmasked addition.
        (*gp::cr.add(3) as u32).wrapping_add((linear >> 22) * 4)
    };
    let pde = table(directory, pae)?;
    if pde & 1 == 0 || user && pde & 4 == 0 {
        return Err(CaptureError::Unreadable);
    }
    if pde & 128 != 0 && cr4 & cpu::CR4_PSE != 0 {
        return Ok(if pae {
            pde & 0xFFE00000 | linear & 0x1FFFFF
        } else {
            pde & 0xFFC00000 | linear & 0x3FFFFF
        });
    }
    let offset = if pae { (linear >> 12 & 511) * 8 } else { (linear >> 12 & 1023) * 4 };
    let pte = table((pde & !4095).wrapping_add(offset), pae)?;
    if pte & 1 == 0 || user && pte & 4 == 0 {
        return Err(CaptureError::Unreadable);
    }
    Ok(pte & !4095 | linear & 4095)
}
/// Online execution may skip instruction-byte reads only after the CPU has
/// established every covered page's visible translation. Do not eagerly walk
/// secondary/unreachable pages and alter their accessed bits during admission.
pub unsafe fn mappings_cached(snapshot: &ImmutableCodeSnapshot) -> bool {
    let mask = cpu::TLB_VALID | if *gp::cpl == 3 { cpu::TLB_NO_USER } else { 0 };
    snapshot.mappings.iter().all(|mapping| {
        let cached = cpu::tlb_data[(mapping.linear.0 >> 12) as usize];
        cached & mask == cpu::TLB_VALID
            && ((cached as u32 & !4095) ^ mapping.linear.0).wrapping_sub(memory::mem8 as u32)
                == mapping.physical.0
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CachedMatch {
    Match,
    Unavailable,
    Stale,
}
/// Fast execution-time validation for already-visible code pages. This preserves
/// raw/unnotified SMC detection by comparing the authoritative physical RAM bytes,
/// but avoids allocating a new snapshot or walking page tables on a hot admission.
/// A missing cached translation is not stale: callers may fall back to capture()
/// before the architectural fetch, or simply decline admission afterward.
pub unsafe fn cached_match(
    linear: u32,
    snapshot: &ImmutableCodeSnapshot,
) -> CachedMatch {
    if !mappings_cached(snapshot) {
        return CachedMatch::Unavailable;
    }
    // Most captures fit one page. Keep mapping admission first, then compare
    // that page directly without the generic chunk loop or per-owner metadata.
    if let [mapping] = snapshot.mappings.as_slice() {
        let page_offset = linear & 4095;
        if snapshot.bytes.is_empty() || snapshot.bytes.len() > (4096 - page_offset) as usize
            || mapping.linear.0 != linear & !4095 {
            return CachedMatch::Stale;
        }
        let Some(physical) = mapping.physical.0.checked_add(page_offset) else {
            return CachedMatch::Stale;
        };
        return match ram(physical, snapshot.bytes.len()) {
            Ok(current) if current == snapshot.bytes.as_slice() => CachedMatch::Match,
            _ => CachedMatch::Stale,
        };
    }
    let mut offset = 0usize;
    for mapping in &snapshot.mappings {
        if offset >= snapshot.bytes.len() {
            return CachedMatch::Stale;
        }
        let address = linear.wrapping_add(offset as u32);
        if mapping.linear.0 != address & !4095 {
            return CachedMatch::Stale;
        }
        let page_offset = (address & 4095) as usize;
        let chunk = (4096 - page_offset).min(snapshot.bytes.len() - offset);
        let Some(physical) = mapping.physical.0.checked_add(page_offset as u32) else {
            return CachedMatch::Stale;
        };
        let Ok(current) = ram(physical, chunk) else {
            return CachedMatch::Stale;
        };
        if current != &snapshot.bytes[offset..offset + chunk] {
            return CachedMatch::Stale;
        }
        offset += chunk;
    }
    if offset == snapshot.bytes.len() {
        CachedMatch::Match
    } else {
        CachedMatch::Stale
    }
}
pub unsafe fn capture(linear: u32, length: usize) -> Result<ImmutableCodeSnapshot, CaptureError> {
    if length == 0 || length > 15 * 128 {
        return Err(CaptureError::Size);
    }
    let mut snapshot = ImmutableCodeSnapshot {
        bytes: Vec::with_capacity(length),
        dependencies: vec![],
        mappings: vec![],
    };
    while snapshot.bytes.len() < length {
        let address = linear.wrapping_add(snapshot.bytes.len() as u32);
        let physical = translate(address)?;
        let chunk = (4096 - (address & 4095) as usize).min(length - snapshot.bytes.len());
        let page = PhysicalAddress(physical & !4095);
        snapshot.mappings.push(CodeMapping {
            linear: LinearAddress(address & !4095),
            physical: page,
        });
        if !snapshot.dependencies.iter().any(|d| d.page == page) {
            snapshot
                .dependencies
                .push(CodeDependency { page, version: 1 });
        }
        snapshot.bytes.extend_from_slice(ram(physical, chunk)?);
    }
    Ok(snapshot)
}
