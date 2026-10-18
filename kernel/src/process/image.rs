//! Parsing and frame-backed loading of read-only embedded user images.

use core::ptr;

use crate::{
  arch,
  memory::{FRAME_SIZE, FrameAllocator, PagePermissions, VirtAddr},
};

/// Byte length of the four-field image header.
const HEADER_SIZE: usize = 32;

/// Read-only executable bytes and their linked virtual addresses.
pub(super) struct Image<'a> {
  /// First user virtual address occupied by the payload.
  pub(super) base: VirtAddr,

  /// User instruction address at which execution starts.
  pub(super) entry: VirtAddr,

  /// Payload bytes, excluding the image header.
  bytes: &'a [u8],
}

impl<'a> Image<'a> {
  /// Validates a BBUSER01 image bounded by `limit`, excluding that address.
  ///
  /// The header contains an eight-byte magic, then little-endian u64 load
  /// address, entry address, and payload length. The load address must be
  /// page-aligned and exclude the null page. Entry must be halfword-aligned
  /// and lie inside the payload. Complete mapped pages must fit below limit.
  ///
  /// # Errors
  ///
  /// Returns an error for missing, truncated, inconsistent, or out-of-range data.
  pub(super) fn parse(bytes: &'a [u8], limit: usize) -> Result<Self, super::LoadError> {
    if bytes.is_empty() {
      return Err(super::LoadError::MissingImage);
    }

    let invalid = || super::LoadError::InvalidImage;
    let header = bytes.get(..HEADER_SIZE).ok_or_else(invalid)?;
    if header.get(..8) != Some(b"BBUSER01".as_slice()) {
      return Err(invalid());
    }

    let field = |range: core::ops::Range<usize>| {
      let value = u64::from_le_bytes(header.get(range)?.try_into().ok()?);
      usize::try_from(value).ok()
    };
    let base = field(8..16).ok_or_else(invalid)?;
    let entry = field(16..24).ok_or_else(invalid)?;
    let size = field(24..32).ok_or_else(invalid)?;
    let payload = bytes.get(HEADER_SIZE..).ok_or_else(invalid)?;
    let end = base.checked_add(size).ok_or_else(invalid)?;
    let mapped_end = end.checked_add(FRAME_SIZE - 1).ok_or_else(invalid)? & !(FRAME_SIZE - 1);

    if size == 0
      || size != payload.len()
      || base < FRAME_SIZE
      || !base.is_multiple_of(FRAME_SIZE)
      || !(base..end).contains(&entry)
      || !entry.is_multiple_of(2)
      || mapped_end > limit
    {
      return Err(invalid());
    }

    Ok(Self {
      base: VirtAddr::new(base),
      entry: VirtAddr::new(entry),
      bytes: payload,
    })
  }

  /// Copies the payload into fresh user-readable, executable, non-writable pages.
  ///
  /// Unused bytes in the final page are zeroed. The caller must hold exclusive
  /// paging/frame access with local interrupts disabled and the supplied root
  /// active on its owning hart.
  ///
  /// # Errors
  ///
  /// Returns an error for unavailable frames or failed mappings. Installed
  /// mappings and consumed frames remain allocated after partial failure.
  pub(super) fn load(
    &self,
    paging: &mut arch::KernelPaging,
    frames: &mut impl FrameAllocator,
  ) -> Result<(), super::LoadError> {
    let mut address = self.base;
    for bytes in self.bytes.chunks(FRAME_SIZE) {
      map_page(paging, frames, address, bytes, PagePermissions::ReadExecute)?;
      address =
        address
          .checked_add(FRAME_SIZE)
          .ok_or(arch::PagingError::VirtualAddressNotUser {
            address: address.as_usize(),
          })?;
    }
    Ok(())
  }
}

/// Initializes and maps a fresh user page with the requested access.
///
/// `bytes` must fit in one frame. Remaining bytes are zeroed before exposing
/// the page to user mode. The caller holds exclusive paging/frame access with
/// interrupts disabled on the active root's owning hart.
///
/// # Errors
///
/// Returns an error if contents exceed one frame, frames are exhausted, or
/// frame access or mapping fails.
pub(super) fn map_page(
  paging: &mut arch::KernelPaging,
  frames: &mut impl FrameAllocator,
  address: VirtAddr,
  bytes: &[u8],
  permissions: PagePermissions,
) -> Result<(), super::LoadError> {
  if bytes.len() > FRAME_SIZE {
    return Err(super::LoadError::InvalidImage);
  }
  let mut frame = frames
    .allocate()
    .ok_or(arch::PagingError::OutOfPhysicalFrames)?;
  paging.zero_frame(&mut frame)?;
  if !bytes.is_empty() {
    paging.with_frame(frame.frame(), |alias| {
      let destination = ptr::with_exposed_provenance_mut::<u8>(alias.as_usize());
      // SAFETY:
      // This fresh frame is exclusively owned and mapped writable for the
      // closure. The source is immutable image storage, and bytes fits within
      // the frame, so both regions are valid and cannot overlap.
      unsafe {
        ptr::copy_nonoverlapping(bytes.as_ptr(), destination, bytes.len());
      }
    })?;
  }
  paging
    .map_user_page(address, frame, permissions, frames)
    .map_err(super::LoadError::Paging)
}
