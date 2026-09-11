//! Core physical memory abstractions.
//!
//! This module provides the common physical-memory vocabulary used by the
//! kernel's memory-management code.
//!
//! Physical addresses are represented by [`PhysAddr`], while [`PhysRange`]
//! represents non-empty half-open regions of physical address space. Physical
//! frame representation and boot-time frame allocation are implemented by the
//! [`frame`] submodule.

mod address;
mod frame;

pub(crate) use address::PhysAddr;
pub(crate) use frame::{BootFrameAllocator, BootFrameAllocatorError, PhysFrame};

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
  /// Linker-defined symbol marking the start of the kernel's boot-time
  /// physical memory range.
  static _kernel_start: u8;

  /// Linker-defined symbol marking the end of the kernel's boot-time
  /// physical memory range.
  static _kernel_end: u8;
}

/// Returns the physical memory range reserved for the kernel at boot.
///
/// The range is derived from the linker-defined [`_kernel_start`] and
/// [`_kernel_end`] boundaries and includes the linked kernel sections and
/// statically reserved boot stack.
///
/// # Panics
///
/// Panics if the linker-provided boundaries do not describe a non-empty range.
#[must_use]
#[expect(
  clippy::expect_used,
  reason = "the linker script places _kernel_end strictly after _kernel_start"
)]
pub(crate) fn kernel_range() -> PhysRange {
  let start = PhysAddr::new(core::ptr::addr_of!(_kernel_start).addr());
  let end = PhysAddr::new(core::ptr::addr_of!(_kernel_end).addr());

  PhysRange::new(start, end).expect("linker must produce a non-empty kernel image")
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
