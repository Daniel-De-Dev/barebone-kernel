//! Core kernel memory abstractions.
//!
//! This module provides the common physical-memory vocabulary used by the
//! kernel's memory-management code.
//!
//! Physical and virtual addresses are represented by [`PhysAddr`] and
//! [`VirtAddr`]. Physical frame representation and boot-time frame
//! allocation are implemented by the [`frame`] submodule.

mod address;
mod frame;

pub(crate) use address::{PhysAddr, VirtAddr};
pub(crate) use frame::{AllocatedFrame, BootFrameAllocator, PhysFrame};

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

/// A non-empty half-open virtual address range `[start, end)`.
///
/// A `VirtRange` guarantees that `start` is strictly less than `end`. The
/// start address is included in the range, while the end address is excluded.
#[derive(Clone, Copy, Debug)]
pub(crate) struct VirtRange {
  /// Inclusive start address.
  start: VirtAddr,

  /// Exclusive end address.
  end: VirtAddr,
}

/// Linker-defined virtual ranges of the higher-half kernel sections.
///
/// Each range includes its start and excludes its end. Section starts are
/// page-aligned, but ends describe the actual section contents and may require
/// rounding up when mapping complete pages.
///
/// Empty sections have equal linker bounds and are represented by `None`.
#[derive(Debug)]
pub(crate) struct KernelSections {
  /// Virtual origin corresponding to the physical kernel image start.
  pub(crate) virtual_start: VirtAddr,

  /// Executable kernel code.
  pub(crate) text: Option<VirtRange>,

  /// Read-only kernel constants.
  pub(crate) rodata: Option<VirtRange>,

  /// Initialized writable kernel data.
  pub(crate) data: Option<VirtRange>,

  /// Zero-initialized writable kernel data.
  pub(crate) bss: Option<VirtRange>,

  /// Reserved boot stack.
  pub(crate) stack: Option<VirtRange>,
}

unsafe extern "C" {
  /// Linker-defined symbol marking the start of the kernel's complete virtual
  /// boot-time footprint.
  static _kernel_virtual_start: u8;

  /// Linker-defined symbol marking the end of the kernel's complete virtual
  /// boot-time footprint.
  static _kernel_virtual_end: u8;

  /// Inclusive virtual start of executable kernel code.
  static _text_start: u8;

  /// Exclusive virtual end of executable kernel code.
  static _text_end: u8;

  /// Inclusive virtual start of read-only kernel constants.
  static _rodata_start: u8;

  /// Exclusive virtual end of read-only kernel constants.
  static _rodata_end: u8;

  /// Inclusive virtual start of initialized writable kernel data.
  static _data_start: u8;

  /// Exclusive virtual end of initialized writable kernel data.
  static _data_end: u8;

  /// Inclusive virtual start of zero-initialized kernel data.
  static _bss_start: u8;

  /// Exclusive virtual end of zero-initialized kernel data.
  static _bss_end: u8;

  /// Inclusive virtual start of the reserved boot stack.
  static _stack_start: u8;

  /// Exclusive virtual end of the reserved boot stack.
  static _stack_end: u8;
}

/// Returns the linker-defined virtual boundaries of the kernel sections.
///
/// Only symbol addresses are taken. The linker script guarantees ordered
/// ranges with page-aligned starts. Empty sections have equal bounds.
#[must_use]
pub(crate) fn kernel_sections() -> KernelSections {
  let range = |start: *const u8, end: *const u8| {
    VirtRange::new(VirtAddr::new(start.addr()), VirtAddr::new(end.addr()))
  };

  KernelSections {
    virtual_start: VirtAddr::new(core::ptr::addr_of!(_kernel_virtual_start).addr()),
    text: range(
      core::ptr::addr_of!(_text_start),
      core::ptr::addr_of!(_text_end),
    ),
    rodata: range(
      core::ptr::addr_of!(_rodata_start),
      core::ptr::addr_of!(_rodata_end),
    ),
    data: range(
      core::ptr::addr_of!(_data_start),
      core::ptr::addr_of!(_data_end),
    ),
    bss: range(
      core::ptr::addr_of!(_bss_start),
      core::ptr::addr_of!(_bss_end),
    ),
    stack: range(
      core::ptr::addr_of!(_stack_start),
      core::ptr::addr_of!(_stack_end),
    ),
  }
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

// TODO: Merge with PhysRange as the only difference so far was the type used.
// Maybe define a trait which can be derived for both?
impl VirtRange {
  /// Constructs a virtual range from its inclusive start and exclusive end.
  ///
  /// Returns `None` if `start` is greater than or equal to `end`.
  #[must_use]
  pub(crate) const fn new(start: VirtAddr, end: VirtAddr) -> Option<Self> {
    if start.as_usize() >= end.as_usize() {
      return None;
    }

    Some(Self { start, end })
  }

  /// Constructs a virtual range beginning at `start` and spanning `size`
  /// bytes.
  ///
  /// Returns `None` if `size` is zero or if computing the end address would
  /// overflow the virtual address representation.
  #[must_use]
  pub(crate) const fn from_start_size(start: VirtAddr, size: usize) -> Option<Self> {
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
  pub(crate) const fn start(self) -> VirtAddr {
    self.start
  }

  /// Returns the exclusive end address.
  #[must_use]
  pub(crate) const fn end(self) -> VirtAddr {
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
