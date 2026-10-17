//! Downward-growing kernel heap with eagerly backed pages.
//!
//! Allocation consumes virtual space from the top of an exclusively reserved
//! range. Page backing is supplied through [`HeapBacking`]. Allocations and
//! their mappings remain reserved after this allocator is dropped.

use core::{alloc::Layout, fmt, ptr::NonNull};

use super::{VirtAddr, VirtRange, frame::FRAME_SIZE};

/// Supplies persistent RAM pages for the kernel heap.
///
/// # Safety
///
/// For a page-aligned, previously unmapped address in the reserved range,
/// success must map the complete [`FRAME_SIZE`]-sized page to exclusively
/// allocated, zero-initialized RAM with supervisor read/write permissions and
/// no execute access.
/// The mapping must remain valid for all allocations using it, including after
/// this backing object is dropped or a later request fails. On failure, the
/// requested leaf must remain unmapped. Previously backed pages remain valid.
pub(crate) unsafe trait HeapBacking {
  /// Error returned when a heap page cannot be backed.
  type Error;

  /// Backs one previously unmapped page with zero-initialized RAM.
  ///
  /// # Errors
  ///
  /// Returns an error if the page cannot be allocated, initialized, or mapped.
  fn map_page(&mut self, address: VirtAddr) -> Result<(), Self::Error>;
}

/// Errors during heap initialization or allocation.
#[derive(Debug)]
pub(crate) enum HeapError<E> {
  /// The range is not page-aligned or includes the null address.
  InvalidRange {
    /// Requested heap range.
    range: VirtRange,
  },

  /// The requested allocation size is zero.
  ZeroSizedAllocation,

  /// The requested size and alignment do not fit the remaining virtual space.
  Exhausted,

  /// Physical page backing failed.
  Backing(E),
}

/// Monotonic allocator for an exclusively reserved virtual range.
///
/// Returned blocks meet their [`Layout`] alignment and never overlap. All pages
/// covering a block are backed before its pointer is returned. The allocation
/// cursor advances only on success. Pages backed before a failure are retained
/// and reused by a later attempt. Allocated virtual space is never reclaimed.
#[must_use = "retain the heap state to allocate from its remaining virtual space"]
pub(crate) struct KernelHeap {
  /// Complete page-aligned virtual reservation.
  range: VirtRange,

  /// Exclusive upper bound for the next downward allocation.
  cursor: VirtAddr,

  /// Inclusive lower bound of the backed suffix ending at `range.end()`.
  mapped_start: VirtAddr,
}

impl fmt::Debug for KernelHeap {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    formatter
      .debug_struct("KernelHeap")
      .field("range", &self.range)
      .field("cursor", &self.cursor)
      .field("mapped_start", &self.mapped_start)
      .finish()
  }
}

impl KernelHeap {
  /// Initializes an empty heap and backs its highest page.
  ///
  /// # Safety
  ///
  /// The entire range must be exclusively reserved, initially unmapped, and
  /// usable for kernel heap pages. No other allocator may issue allocations
  /// from it, even after this value is dropped. Every subsequent backing object
  /// must control the same reservation and preserve its installed mappings.
  ///
  /// # Errors
  ///
  /// Returns an error for an invalid range or if its first page cannot be
  /// backed. Backing failures may permanently consume physical frames.
  pub(crate) unsafe fn new<B: HeapBacking>(
    range: VirtRange,
    backing: &mut B,
  ) -> Result<Self, HeapError<B::Error>> {
    let start = range.start().as_usize();
    let end = range.end().as_usize();

    if start == 0 || !start.is_multiple_of(FRAME_SIZE) || !end.is_multiple_of(FRAME_SIZE) {
      return Err(HeapError::InvalidRange { range });
    }

    let mut heap = Self {
      range,
      cursor: range.end(),
      mapped_start: range.end(),
    };

    let first_page = end
      .checked_sub(FRAME_SIZE)
      .ok_or(HeapError::InvalidRange { range })?;

    heap.back_to(VirtAddr::new(first_page), backing)?;

    Ok(heap)
  }

  /// Allocates a block, backing additional pages before returning its pointer.
  ///
  /// New pages are zero-initialized. Alignment padding is consumed along with
  /// the block. Use a backing object for this heap's original reservation.
  ///
  /// # Errors
  ///
  /// Returns an error for a zero-sized [`Layout`], insufficient virtual space, or
  /// failure to back a required page. No pointer is issued on failure, and the
  /// allocation cursor is unchanged. Successfully backed pages are retained.
  #[expect(
    clippy::arithmetic_side_effects,
    reason = "Layout alignment and the constant frame size are non-zero powers of two"
  )]
  pub(crate) fn allocate<B: HeapBacking>(
    &mut self,
    layout: Layout,
    backing: &mut B,
  ) -> Result<NonNull<u8>, HeapError<B::Error>> {
    if layout.size() == 0 {
      return Err(HeapError::ZeroSizedAllocation);
    }

    let address = self
      .cursor
      .as_usize()
      .checked_sub(layout.size())
      .map(|address| address & !(layout.align() - 1))
      .filter(|address| *address >= self.range.start().as_usize())
      .ok_or(HeapError::Exhausted)?;

    let pointer = NonNull::new(core::ptr::with_exposed_provenance_mut::<u8>(address))
      .ok_or(HeapError::Exhausted)?;
    let first_page = VirtAddr::new(address & !(FRAME_SIZE - 1));

    self.back_to(first_page, backing)?;
    self.cursor = VirtAddr::new(address);

    Ok(pointer)
  }

  /// Extends the backed interval downward, retaining progress on failure.
  ///
  /// # Errors
  ///
  /// Returns an error if a required page cannot be backed.
  fn back_to<B: HeapBacking>(
    &mut self,
    start: VirtAddr,
    backing: &mut B,
  ) -> Result<(), HeapError<B::Error>> {
    while self.mapped_start.as_usize() > start.as_usize() {
      let address = self
        .mapped_start
        .as_usize()
        .checked_sub(FRAME_SIZE)
        .ok_or(HeapError::Exhausted)?;
      let page = VirtAddr::new(address);

      backing.map_page(page).map_err(HeapError::Backing)?;
      self.mapped_start = page;
    }

    Ok(())
  }
}
