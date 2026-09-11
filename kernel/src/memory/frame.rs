//! Physical memory frame representation and alignment helpers.
//!
//! This module defines the kernel's fixed-size physical frame abstraction used
//! by early physical memory management.
//!
//! A physical frame is [`FRAME_SIZE`] bytes in size and must begin at a
//! [`FRAME_SIZE`]-aligned physical address. [`PhysFrame`] guarantees both that
//! its start address is correctly aligned and that its complete address range
//! is representable.
//!
//! Frame alignment helpers are also provided for rounding arbitrary physical
//! addresses to frame boundaries.
//!
//! Boot-time frame allocation is implemented by the `boot_allocator`
//! submodule.

mod boot_allocator;

pub(crate) use boot_allocator::{BootFrameAllocator, BootFrameAllocatorError};

use super::{PhysAddr, PhysRange};

/// Size of one physical memory frame in bytes.
pub(crate) const FRAME_SIZE: usize = 4096;

/// One fixed-size physical memory frame.
///
/// A `PhysFrame` guarantees that its start address is aligned to
/// [`FRAME_SIZE`] and that the complete frame range is representable.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PhysFrame {
  /// Frame-aligned physical start address.
  start: PhysAddr,
}

impl PhysFrame {
  /// Constructs a physical frame beginning at `start`.
  ///
  /// Returns `None` if `start` is not frame-aligned or if the complete frame
  /// range cannot be represented.
  #[must_use]
  pub(crate) const fn from_start(start: PhysAddr) -> Option<Self> {
    if !start.as_usize().is_multiple_of(FRAME_SIZE) {
      return None;
    }

    if start.checked_add(FRAME_SIZE).is_none() {
      return None;
    }

    Some(Self { start })
  }

  /// Returns the physical start address of this frame.
  #[must_use]
  pub(crate) const fn start_address(self) -> PhysAddr {
    self.start
  }

  /// Returns the physical address range occupied by this frame.
  ///
  /// # Panics
  ///
  /// Panics if the invariants guaranteed by [`PhysFrame`] are violated.
  #[must_use]
  #[expect(
    clippy::expect_used,
    reason = "PhysFrame construction guarantees a representable, non-empty frame range"
  )]
  pub(crate) const fn range(self) -> PhysRange {
    PhysRange::from_start_size(self.start, FRAME_SIZE)
      .expect("PhysFrame guarantees a representable non-empty physical range")
  }
}

/// Rounds a physical address up to the nearest frame boundary.
///
/// If `address` is already frame-aligned, it is returned unchanged.
///
/// Returns `None` if rounding upward would overflow the physical address
/// representation.
#[must_use]
pub(super) fn align_up(address: PhysAddr) -> Option<PhysAddr> {
  let value = address.as_usize();
  let remainder = value % FRAME_SIZE;

  if remainder == 0 {
    return Some(address);
  }

  let adjustment = FRAME_SIZE.checked_sub(remainder)?;

  address.checked_add(adjustment)
}

/// Rounds a physical address down to the nearest frame boundary.
///
/// If `address` is already frame-aligned, it is returned unchanged.
#[must_use]
#[expect(
  clippy::arithmetic_side_effects,
  reason = "`value % FRAME_SIZE` can never exceed `value`, so subtraction cannot underflow"
)]
pub(super) const fn align_down(address: PhysAddr) -> PhysAddr {
  let value = address.as_usize();

  PhysAddr::new(value - value % FRAME_SIZE)
}
