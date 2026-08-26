//! Construction and validation of a flattened devicetree blob.
//!
//! This module defines [`Fdt`], the top-level validated view of a DTB, and owns
//! the unsafe boundary through which raw DTB memory enters the parser.
//!
//! Successful construction establishes a bounded immutable view of the blob and
//! validates the data needed by the views retained in [`Fdt`].
//!
//! Implementation based on [Devicetree Specification v0.4](https://www.devicetree.org/).

use core::slice;

use crate::{
  error::Error,
  header::{HEADER_SIZE, Header},
  strings::Strings,
  structure::Structure,
};

/// Errors encountered while establishing the byte range occupied by a DTB.
#[derive(Debug, PartialEq)]
pub enum BlobError {
  /// The supplied DTB pointer is null.
  NullPointer,

  /// The DTB begins at an address that does not satisfy the required alignment.
  Misaligned {
    /// Address supplied for the beginning of the DTB.
    address: usize,

    /// Required byte alignment.
    required_alignment: usize,
  },

  /// The end address of the DTB cannot be represented without wrapping.
  AddressRangeOverflow {
    /// Address at which the DTB begins.
    address: usize,

    /// Size of the DTB byte range.
    size: usize,
  },

  /// The DTB is too large to represent as a Rust slice on this target.
  #[cfg(target_pointer_width = "32")]
  TooLarge {
    /// Declared total size of the DTB, in bytes.
    total_size: usize,

    /// Maximum representable slice size, in bytes.
    maximum: usize,
  },
}

/// A validated view of a flattened devicetree blob.
///
/// `Fdt` borrows the underlying DTB memory for `'a` and retains the validated
/// views needed to access its contents. Construction is performed through
/// [`Fdt::from_ptr`].
///
/// Structural validity does not imply that the represented devicetree satisfies
/// the semantic requirements of any particular device or binding.
#[derive(Debug)]
pub struct Fdt<'a> {
  /// Validated information decoded from the DTB header.
  header: Header,

  /// Validated structure block.
  structure: Structure<'a>,

  /// Strings block referenced by the structure block.
  strings: Strings<'a>,
}

impl<'a> Fdt<'a> {
  /// Constructs and validates an FDT backed by memory beginning at `ptr`.
  ///
  /// On success, the returned [`Fdt`] borrows the underlying DTB memory for
  /// `'a`. Malformed or unsupported blob contents are reported through
  /// [`Error`].
  ///
  /// # Safety
  ///
  /// The caller must guarantee that `ptr` is valid for reading at least
  /// [`HEADER_SIZE`] initialized bytes. If those bytes describe a header that
  /// is accepted by the parser, the complete byte range declared by that header
  /// must be initialized, readable, contiguous, contained within one allocation,
  /// and remain valid and unmodified for `'a`.
  ///
  /// # Errors
  ///
  /// Returns [`Error::Blob`] if the DTB memory range cannot be represented
  /// safely, or a validation error if the encoded DTB is malformed or
  /// unsupported.
  pub unsafe fn from_ptr(ptr: *const u8) -> Result<Self, Error> {
    if ptr.is_null() {
      return Err(BlobError::NullPointer.into());
    }

    let address = ptr.addr();

    if !address.is_multiple_of(8) {
      return Err(
        BlobError::Misaligned {
          address,
          required_alignment: 8,
        }
        .into(),
      );
    }

    // SAFETY: The caller guarantees that `ptr` points to at least
    // `HEADER_SIZE` initialized bytes valid for reads.
    let header_bytes = unsafe { ptr.cast::<[u8; HEADER_SIZE]>().read() };

    let header = Header::new(&header_bytes)?;
    let total_size = header.total_size();

    // `totalsize` is encoded as a `u32`. On 32-bit targets it may exceed
    // `isize::MAX`, violating the maximum size permitted for a Rust slice.
    // On 64-bit targets every possible value satisfies `u32::MAX < isize::MAX`.
    #[cfg(target_pointer_width = "32")]
    if total_size > isize::MAX as usize {
      return Err(
        BlobError::TooLarge {
          total_size,
          maximum: isize::MAX as usize,
        }
        .into(),
      );
    }

    address
      .checked_add(total_size)
      .ok_or(BlobError::AddressRangeOverflow {
        address,
        size: total_size,
      })?;

    // SAFETY:
    // - `ptr` is non-null.
    // - The caller guarantees that the complete range is initialized, readable,
    //   contained within one allocation, and remains valid and unmodified for
    //   `'a`.
    let bytes: &'a [u8] = unsafe { slice::from_raw_parts(ptr, total_size) };

    let strings_bytes = &bytes[header.strings_range()];
    let structure_bytes = &bytes[header.structure_range()];

    let strings = Strings::new(strings_bytes);
    let structure = Structure::new(structure_bytes, &strings)?;

    Ok(Self {
      header,
      structure,
      strings,
    })
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::{error::Error, header::HeaderError, structure::StructureError};

  // TODO: Look into elegantly solving the repetition of helper functions and
  // constants. (Same stuff defined in `header.rs` & `structure.rs`)
  const MAGIC: usize = 0;
  const TOTAL_SIZE: usize = 4;
  const STRUCTURE_OFFSET: usize = 8;
  const STRINGS_OFFSET: usize = 12;
  const RESERVATION_OFFSET: usize = 16;
  const VERSION: usize = 20;
  const LAST_COMP_VERSION: usize = 24;
  const BOOT_CPUID_PHYS: usize = 28;
  const STRINGS_SIZE: usize = 32;
  const STRUCTURE_SIZE: usize = 36;

  const FDT_BEGIN_NODE: u32 = 0x1;
  const FDT_END_NODE: u32 = 0x2;
  const FDT_END: u32 = 0x9;

  #[repr(align(8))]
  struct Aligned<const N: usize>([u8; N]);

  fn set_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
  }

  fn minimal_blob() -> Aligned<56> {
    let mut bytes = [0; 56];

    set_u32(&mut bytes, MAGIC, 0xd00d_feed);
    set_u32(&mut bytes, TOTAL_SIZE, 56);
    set_u32(&mut bytes, STRUCTURE_OFFSET, 40);
    set_u32(&mut bytes, STRINGS_OFFSET, 56);
    set_u32(&mut bytes, RESERVATION_OFFSET, 40);
    set_u32(&mut bytes, VERSION, 17);
    set_u32(&mut bytes, LAST_COMP_VERSION, 16);
    set_u32(&mut bytes, BOOT_CPUID_PHYS, 0);
    set_u32(&mut bytes, STRINGS_SIZE, 0);
    set_u32(&mut bytes, STRUCTURE_SIZE, 16);

    set_u32(&mut bytes, 40, FDT_BEGIN_NODE);

    set_u32(&mut bytes, 48, FDT_END_NODE);
    set_u32(&mut bytes, 52, FDT_END);

    Aligned(bytes)
  }

  #[test]
  fn valid_blob_is_constructed() {
    let blob = minimal_blob();

    // SAFETY: `blob` contains the complete initialized DTB and remains alive
    // for the returned `Fdt`.
    let fdt = unsafe { Fdt::from_ptr(blob.0.as_ptr()) }.expect("valid DTB should parse");

    assert_eq!(fdt.header.total_size(), 56);
    assert_eq!(fdt.header.structure_range(), 40..56);
    assert_eq!(fdt.header.strings_range(), 56..56);
  }

  #[test]
  fn null_pointer_is_rejected() {
    let error = unsafe { Fdt::from_ptr(core::ptr::null()) }.unwrap_err();

    assert_eq!(error, Error::Blob(BlobError::NullPointer));
  }

  #[test]
  fn misaligned_pointer_is_rejected() {
    let blob = Aligned([0u8; 64]);

    let ptr = unsafe { blob.0.as_ptr().add(1) };

    let error = unsafe { Fdt::from_ptr(ptr) }.unwrap_err();

    assert_eq!(
      error,
      Error::Blob(BlobError::Misaligned {
        address: ptr.addr(),
        required_alignment: 8,
      })
    );
  }

  #[test]
  fn header_error_is_propagated() {
    let mut blob = minimal_blob();

    set_u32(&mut blob.0, MAGIC, 0xdead_beef);

    let error = unsafe { Fdt::from_ptr(blob.0.as_ptr()) }.unwrap_err();

    assert_eq!(
      error,
      Error::Header(HeaderError::InvalidMagic { found: 0xdead_beef })
    );
  }

  #[test]
  fn structure_error_is_propagated() {
    let mut blob = minimal_blob();

    set_u32(&mut blob.0, 40, FDT_END);

    let error = unsafe { Fdt::from_ptr(blob.0.as_ptr()) }.unwrap_err();

    assert_eq!(
      error,
      Error::Structure(StructureError::ExpectedRootNode {
        offset: 0,
        found: FDT_END,
      })
    );
  }
}
