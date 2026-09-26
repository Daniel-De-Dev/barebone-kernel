//! Core physical memory abstractions.
//!
//! This module provides the common physical-memory vocabulary used by the
//! kernel's memory-management code.
//!
//! Physical and virtual addresses are represented by [`PhysAddr`] and
//! [`VirtAddr`], while [`PhysRange`] represents non-empty half-open regions of
//! physical address space. Physical frame representation and boot-time frame
//! allocation are implemented by the [`frame`] submodule.

mod address;
mod frame;

pub(crate) use address::{PhysAddr, VirtAddr};
pub(crate) use frame::BootFrameAllocator;

/// A non-empty half-open physical address range `[start, end)`.
///
/// A `PhysRange` guarantees that `start` is strictly less than `end`. The
/// start address is included in the range, while the end address is excluded.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PhysRange {
  /// Inclusive start address.
  start: PhysAddr,

  /// Exclusive end address.
  end: PhysAddr,
}

unsafe extern "C" {
  /// Linker-defined symbol marking the start of the kernel's complete virtual
  /// boot-time footprint.
  static _kernel_virtual_start: u8;

  /// Linker-defined symbol marking the end of the kernel's complete virtual
  /// boot-time footprint.
  static _kernel_virtual_end: u8;
}

/// Returns the kernel's complete boot-time memory footprint in bytes.
///
/// The span includes the physical bootstrap reservation, higher-half kernel
/// sections, `.bss`, boot stack, and linker-introduced alignment.
///
/// # Panics
///
/// Panics if the linker does not provide a non-empty virtual kernel range.
#[must_use]
#[expect(
  clippy::expect_used,
  reason = "the linker guarantees a non-empty virtual kernel range"
)]
fn kernel_size() -> usize {
  let start = core::ptr::addr_of!(_kernel_virtual_start).addr();
  let end = core::ptr::addr_of!(_kernel_virtual_end).addr();

  end
    .checked_sub(start)
    .expect("linker must produce a non-empty kernel virtual range")
}

/// Returns the physical memory range reserved for the kernel at boot.
///
/// `physical_start` is supplied by the architecture bootstrap because the
/// higher-half kernel cannot infer its board-specific physical load address
/// from its virtual location alone.
#[must_use]
pub(crate) fn kernel_range(physical_start: PhysAddr) -> Option<PhysRange> {
  PhysRange::from_start_size(physical_start, kernel_size())
}

impl PhysRange {
  /// Constructs a physical range from its inclusive start and exclusive end.
  ///
  /// Returns `None` if `start` is greater than or equal to `end`.
  #[must_use]
  pub(crate) const fn new(start: PhysAddr, end: PhysAddr) -> Option<Self> {
    if start.as_usize() >= end.as_usize() {
      return None;
    }

    Some(Self { start, end })
  }

  /// Constructs a physical range beginning at `start` and spanning `size`
  /// bytes.
  ///
  /// Returns `None` if `size` is zero or if computing the end address would
  /// overflow the physical address representation.
  #[must_use]
  pub(crate) const fn from_start_size(start: PhysAddr, size: usize) -> Option<Self> {
    if size == 0 {
      return None;
    }

    let Some(end) = start.checked_add(size) else {
      return None;
    };

    Self::new(start, end)
  }

  /// Returns the inclusive start address.
  #[must_use]
  pub(crate) const fn start(self) -> PhysAddr {
    self.start
  }

  /// Returns the exclusive end address.
  #[must_use]
  pub(crate) const fn end(self) -> PhysAddr {
    self.end
  }

  /// Returns whether this range overlaps `other`.
  ///
  /// Ranges that only meet at an endpoint do not overlap.
  #[must_use]
  pub(crate) const fn overlaps(self, other: Self) -> bool {
    self.start.as_usize() < other.end.as_usize() && other.start.as_usize() < self.end.as_usize()
  }
}
