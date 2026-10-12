//! Shared frame initialization and page-table access through a temporary alias.
//!
//! The operations here depend only on access to one complete frame at a time.

use core::ptr;

use super::{
  PAGE_SIZE, PTE_VALID, PagingError, intermediate_frame, page_table_entry,
  read_mapped_page_table_entry, write_mapped_page_table_entry,
};
use crate::{
  logging,
  memory::{AllocatedFrame, FrameAllocator, PhysFrame, VirtAddr},
};

/// Temporary read/write access used to initialize RAM and manipulate tables.
///
/// # Safety
///
/// Implementations must map the supplied frame read/write at the address
/// passed to the closure, for its complete duration. The alias must be
/// frame-aligned, and the window must not be changed concurrently. No reference
/// through the alias may outlive the closure. The implementation must remove
/// the temporary mapping before returning normally.
///
/// Callers of table operations must supply valid, exclusively controlled
/// page-table frames. Zeroing instead requires an exclusively allocated frame.
pub(super) unsafe trait FrameAccess {
  /// Executes an operation while the supplied physical frame is accessible.
  ///
  /// # Errors
  ///
  /// Returns an error if the frame cannot be represented or the window is busy.
  fn with_frame<R>(
    &mut self,
    frame: PhysFrame,
    operation: impl FnOnce(VirtAddr) -> R,
  ) -> Result<R, PagingError>;

  /// Zeroes every byte in an exclusively allocated frame.
  ///
  /// # Errors
  ///
  /// Returns an error if temporary frame access cannot be established.
  fn zero_frame(&mut self, frame: &mut AllocatedFrame) -> Result<(), PagingError> {
    self.with_frame(frame.frame(), |address| {
      let pointer = ptr::with_exposed_provenance_mut::<u8>(address.as_usize());

      // SAFETY:
      // FrameAccess maps the complete frame read/write for this closure. The
      // mutable allocation borrow establishes exclusive ownership of its RAM.
      unsafe {
        ptr::write_bytes(pointer, 0, PAGE_SIZE);
      }
    })
  }

  /// Reads one entry, rejecting an index outside the table.
  ///
  /// # Errors
  ///
  /// Returns an error if the frame cannot be accessed or the index is invalid.
  fn read_page_table_entry(
    &mut self,
    frame: PhysFrame,
    index: usize,
  ) -> Result<usize, PagingError> {
    self.with_frame(frame, |address| {
      // SAFETY:
      // The caller supplies a valid table, and FrameAccess maps its complete
      // physical frame for this closure.
      unsafe { read_mapped_page_table_entry(address, index) }
    })?
  }

  /// Writes one entry, rejecting an index outside the table.
  ///
  /// # Errors
  ///
  /// Returns an error if the frame cannot be accessed or the index is invalid.
  fn write_page_table_entry(
    &mut self,
    frame: PhysFrame,
    index: usize,
    entry: usize,
  ) -> Result<(), PagingError> {
    self.with_frame(frame, |address| {
      // SAFETY:
      // The caller exclusively controls this table, and FrameAccess maps its
      // complete physical frame read/write for this closure.
      unsafe { write_mapped_page_table_entry(address, index, entry) }
    })?
  }

  /// Allocates and clears a page table without using a heap.
  ///
  /// # Errors
  ///
  /// Returns an error if no frame remains or it cannot be accessed.
  fn allocate_page_table(
    &mut self,
    frames: &mut impl FrameAllocator,
  ) -> Result<AllocatedFrame, PagingError> {
    let Some(mut frame) = frames.allocate() else {
      return Err(PagingError::OutOfPhysicalFrames);
    };

    self.zero_frame(&mut frame)?;

    logging::trace!(
      Paging,
      "initialized empty page table pa={:#x} bytes={PAGE_SIZE}",
      frame.start_address(),
    );

    Ok(frame)
  }

  /// Follows a child-table link, creating a zeroed table for an empty entry.
  ///
  /// Existing leaf or malformed entries are rejected. Linked table frames
  /// remain permanently reserved by the current monotonic allocator.
  ///
  /// # Errors
  ///
  /// Returns an error if a frame cannot be allocated or accessed, the index is
  /// invalid, or an existing entry is not a valid intermediate table link.
  fn next_table(
    &mut self,
    parent: PhysFrame,
    index: usize,
    frames: &mut impl FrameAllocator,
  ) -> Result<PhysFrame, PagingError> {
    let entry = self.read_page_table_entry(parent, index)?;

    if entry != 0 {
      return intermediate_frame(index, entry);
    }

    let child = self.allocate_page_table(frames)?;
    let entry = page_table_entry(child.start_address(), PTE_VALID)?;

    self.write_page_table_entry(parent, index, entry)?;

    logging::trace!(
      Paging,
      "linked page table parent_pa={:#x} index={index} -> child_pa={:#x} pte={entry:#x}",
      parent.start_address(),
      child.start_address(),
    );

    Ok(child.into_frame())
  }
}
