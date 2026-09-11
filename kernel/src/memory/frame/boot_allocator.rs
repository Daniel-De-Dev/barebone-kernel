//! Boot-time physical frame allocation.
//!
//! This module provides a simple allocator for discovering and
//! allocating physical frames during early kernel initialization.
//!
//! Physical memory is obtained from the FDT `/memory` ranges. Frames
//! overlapping the kernel's boot-time memory range, the DTB, FDT memory
//! reservations, or static `/reserved-memory` regions are excluded from
//! allocation.
//!
//! The allocator processes each memory range independently and only moves
//! forward within it. It does not reclaim frames or track previously allocated
//! frames. FDT-described memory ranges must not overlap, or the same physical
//! frame could be returned more than once.
//!
//! This implementation currently requires a 64-bit target because FDT physical
//! addresses and sizes are represented as `u64` and converted to the kernel's
//! `usize`-based physical address representation.

#[cfg(not(target_pointer_width = "64"))]
compile_error!("the boot frame allocator currently requires a 64-bit target");

use fdt::{Fdt, MemoryRanges};

use super::{PhysFrame, align_down, align_up};
use crate::memory::{PhysAddr, PhysRange};

/// Errors that can prevent construction of a boot frame allocator.
#[derive(Debug)]
pub(crate) enum BootFrameAllocatorError {
  /// Two FDT-described physical memory ranges overlap.
  OverlappingMemoryRanges {
    /// First overlapping physical memory range.
    first: PhysRange,

    /// Second overlapping physical memory range.
    second: PhysRange,
  },
}

/// State for the physical memory range currently being scanned.
///
/// `cursor` is the start of the next candidate frame, while `end` is the
/// exclusive end of the frame-aligned portion of the range.
///
/// Both addresses are frame-aligned and `cursor` is strictly less than `end`.
/// A `CurrentMemoryRange` always contains at least one complete candidate
/// frame.
#[derive(Clone, Copy, Debug)]
struct CurrentMemoryRange {
  /// Frame-aligned address of the next candidate frame.
  cursor: PhysAddr,

  /// Frame-aligned exclusive end address of the range.
  end: PhysAddr,
}

/// Monotonic physical frame allocator used during early kernel initialization.
///
/// The allocator retains the FDT so reservation information can be consulted
/// while its memory ranges are traversed.
///
/// `'fdt` is the lifetime of the borrowed [`Fdt`] instance, while `'dtb` is the
/// lifetime of the underlying Device Tree Blob memory referenced by the FDT.
pub(crate) struct BootFrameAllocator<'fdt, 'dtb> {
  /// FDT used to discover reserved physical-memory regions.
  fdt: &'fdt Fdt<'dtb>,

  /// Remaining FDT-described physical memory ranges to traverse.
  memory_ranges: MemoryRanges<'dtb>,

  /// Memory range currently being scanned for allocatable frames.
  current: Option<CurrentMemoryRange>,

  /// Physical memory reserved for the kernel at boot.
  kernel: PhysRange,

  /// Physical memory occupied by the Device Tree Blob.
  dtb: PhysRange,
}

/// Converts a validated FDT-described address and size into a physical range.
///
/// FDT range validation guarantees that the range is non-empty and has a
/// representable `u64` end address. This module's 64-bit target requirement
/// guarantees that its address and size are representable as `usize`.
///
/// # Panics
///
/// Panics if the range invariants established by FDT validation are violated.
#[expect(
  clippy::expect_used,
  reason = "64-bit targets represent all u64 FDT values and FDT validation guarantees a non-empty representable range"
)]
fn fdt_range(address: u64, size: u64) -> PhysRange {
  let address =
    usize::try_from(address).expect("64-bit targets can represent FDT u64 physical addresses");

  let size = usize::try_from(size).expect("64-bit targets can represent FDT u64 physical sizes");

  PhysRange::from_start_size(PhysAddr::new(address), size)
    .expect("validated FDT range must have a representable non-empty end")
}

/// Validates the memory-layout requirement of the boot frame allocator.
///
/// Because memory ranges are processed independently and previously allocated
/// frames are not tracked, FDT-described memory ranges must not overlap.
///
/// # Errors
///
/// Returns [`BootFrameAllocatorError::OverlappingMemoryRanges`] if any two
/// physical memory ranges described by the FDT overlap.
fn validate_memory_ranges(fdt: &Fdt<'_>) -> Result<(), BootFrameAllocatorError> {
  for (index, first) in fdt.memory_ranges().enumerate() {
    let first = fdt_range(first.address(), first.size());

    let Some(next_index) = index.checked_add(1) else {
      return Ok(());
    };

    for second in fdt.memory_ranges().skip(next_index) {
      let second = fdt_range(second.address(), second.size());

      if first.overlaps(second) {
        return Err(BootFrameAllocatorError::OverlappingMemoryRanges { first, second });
      }
    }
  }

  Ok(())
}

impl<'fdt, 'dtb> BootFrameAllocator<'fdt, 'dtb> {
  /// Constructs a boot frame allocator.
  ///
  /// `kernel` and `dtb` describe physical ranges that must be excluded from
  /// allocation.
  ///
  /// # Errors
  ///
  /// Returns [`BootFrameAllocatorError::OverlappingMemoryRanges`] if two
  /// FDT-described physical memory ranges overlap.
  pub(crate) fn new(
    fdt: &'fdt Fdt<'dtb>,
    kernel: PhysRange,
    dtb: PhysRange,
  ) -> Result<Self, BootFrameAllocatorError> {
    validate_memory_ranges(fdt)?;

    Ok(Self {
      fdt,
      memory_ranges: fdt.memory_ranges(),
      current: None,
      kernel,
      dtb,
    })
  }

  /// Allocates the next available physical frame.
  ///
  /// Allocation proceeds forward from the current memory-range cursor, skipping
  /// reserved regions as necessary. When the current range is exhausted,
  /// allocation continues from the next suitable FDT-described memory range.
  ///
  /// Successfully returned frames are permanently consumed and will not be
  /// returned again.
  ///
  /// Returns `None` when no remaining memory range contains an allocatable frame.
  ///
  /// # Panics
  ///
  /// Panics if the frame-alignment invariants maintained by the allocator are
  /// violated.
  #[expect(
    clippy::expect_used,
    reason = "CurrentMemoryRange guarantees that candidate cursors are frame-aligned and can contain a complete frame"
  )]
  pub(crate) fn allocate(&mut self) -> Option<PhysFrame> {
    loop {
      if self.current.is_none() && !self.advance_memory_range() {
        return None;
      }

      let current = self.current?;

      let frame =
        PhysFrame::from_start(current.cursor).expect("allocator cursor must remain frame aligned");

      let frame_range = frame.range();

      if let Some(reserved) = self.overlapping_reservation(frame_range) {
        let Some(next) = align_up(reserved.end()) else {
          self.current = None;
          continue;
        };

        if next.as_usize() >= current.end.as_usize() {
          self.current = None;
        } else {
          self.current = Some(CurrentMemoryRange {
            cursor: next,
            end: current.end,
          });
        }

        continue;
      }

      let next = frame_range.end();

      if next.as_usize() >= current.end.as_usize() {
        self.current = None;
      } else {
        self.current = Some(CurrentMemoryRange {
          cursor: next,
          end: current.end,
        });
      }

      return Some(frame);
    }
  }

  /// Advances to the next FDT-described memory range containing at least one
  /// complete frame.
  ///
  /// The range is trimmed to frame-aligned boundaries. Ranges that contain no
  /// complete frame after alignment are skipped.
  ///
  /// Returns `true` after initializing `current` from a suitable range, or
  /// `false` if no such range remains.
  fn advance_memory_range(&mut self) -> bool {
    for range in self.memory_ranges.by_ref() {
      let range = fdt_range(range.address(), range.size());

      let Some(start) = align_up(range.start()) else {
        continue;
      };

      let end = align_down(range.end());

      if start.as_usize() >= end.as_usize() {
        continue;
      }

      self.current = Some(CurrentMemoryRange { cursor: start, end });

      return true;
    }

    false
  }

  /// Returns the first non-empty reserved physical range that overlaps
  /// `candidate`.
  ///
  /// The kernel's boot-time memory range and DTB are checked first, followed by
  /// reservations from the FDT memory reservation block and static
  /// `/reserved-memory` ranges.
  ///
  /// Returns `None` if `candidate` does not overlap any known reserved region.
  fn overlapping_reservation(&self, candidate: PhysRange) -> Option<PhysRange> {
    if candidate.overlaps(self.kernel) {
      return Some(self.kernel);
    }

    if candidate.overlaps(self.dtb) {
      return Some(self.dtb);
    }

    for reservation in self.fdt.memory_reservations() {
      if reservation.size() == 0 {
        continue;
      }

      let range = fdt_range(reservation.address(), reservation.size());

      if candidate.overlaps(range) {
        return Some(range);
      }
    }

    for reservation in self.fdt.reserved_memory_ranges() {
      let range = fdt_range(reservation.address(), reservation.size());

      if candidate.overlaps(range) {
        return Some(range);
      }
    }

    None
  }
}
