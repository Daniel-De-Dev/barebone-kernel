//! Construction of validated flattened devicetree blobs.
//!
//! This module defines [`Fdt`], the top-level view of a flattened devicetree
//! blob, and owns the unsafe boundary through which raw DTB memory enters the
//! parser.
//!
//! [`Fdt::from_ptr`] establishes the blob's memory range and coordinates the
//! validation required before an [`Fdt`] is made available to callers.
//! Guarantees are documented by the validation components that establish them.
//!
//! Implementation based on [Devicetree Specification v0.4](https://www.devicetree.org/).

use core::slice;

use crate::{
  error::Error,
  header::{HEADER_SIZE, Header},
  reservation::Reservations,
  strings::Strings,
  structure::{MemoryRanges, Node, Structure},
};

/// Errors encountered while establishing the byte range occupied by a DTB.
#[derive(Debug, PartialEq, Eq)]
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

/// A validated, read-only view of a flattened devicetree blob.
///
/// `Fdt` borrows the underlying DTB memory for `'a` and retains the validated
/// views used to access its constituent blocks.
///
/// Construction is performed through [`Fdt::from_ptr`]. A successfully
/// constructed `Fdt` guarantees that each retained component satisfies the
/// invariants required by its corresponding view and that the additional
/// devicetree-wide validation performed during construction has succeeded.
///
/// These guarantees do not imply conformance to device-, bus-, or
/// binding-specific semantic requirements.
#[derive(Debug)]
pub struct Fdt<'a> {
  /// Validated information decoded from the DTB header.
  header: Header,

  /// Validated structure block.
  structure: Structure<'a>,

  /// Bounded view of the strings block used to resolve property names.
  strings: Strings<'a>,

  /// Validated memory reservations block.
  reservations: Reservations<'a>,
}

impl<'a> Fdt<'a> {
  /// Constructs an [`Fdt`] from a flattened devicetree blob beginning at `ptr`.
  ///
  /// On success, the returned value satisfies the guarantees documented on
  /// [`Fdt`] and borrows the underlying DTB memory for `'a`.
  ///
  /// # Safety
  ///
  /// If `ptr` is non-null, the caller must guarantee that it is valid for
  /// reading at least [`HEADER_SIZE`] initialized bytes. If those bytes contain
  /// a header accepted by the parser, the complete byte range declared by that
  /// header must be initialized, readable, contiguous, contained within a
  /// single allocation, and remain valid and unmodified for `'a`.
  ///
  /// # Errors
  ///
  /// Returns an [`Error`] if the DTB memory range cannot be represented safely
  /// or if any validation performed during construction fails.
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

    #[expect(
      clippy::indexing_slicing,
      reason = "`strings_range` is validated by `Header::new` against `total_size`"
    )]
    let strings_bytes = &bytes[header.strings_range()];

    #[expect(
      clippy::indexing_slicing,
      reason = "`structure_range` is validated by `Header::new` against `total_size`"
    )]
    let structure_bytes = &bytes[header.structure_range()];

    let strings = Strings::new(strings_bytes);
    let structure = Structure::new(structure_bytes, &strings)?;

    structure.validate_semantics(strings)?;

    // The reservation block does not encode its own size. Bound its candidate
    // extent by the nearest known block beginning after it, or by the end of the
    // DTB if no known block follows it. `Reservations::new` determines the actual
    // end from the terminating `(0, 0)` entry
    let reservation_start = header.reservation_offset();
    let mut reservation_limit = total_size;

    for range in [header.structure_range(), header.strings_range()] {
      if range.start > reservation_start {
        reservation_limit = reservation_limit.min(range.start);
      }
    }

    // `reservation_start` is validated to lie within `total_size`. Every
    // candidate limit is also within `total_size` and is considered only when
    // greater than `reservation_start`. Therefore, this always forms a valid
    // slice range.
    #[expect(
      clippy::indexing_slicing,
      reason = "reservation range is bounded by validated header offsets"
    )]
    let reservation_bytes = &bytes[reservation_start..reservation_limit];

    let reservations = Reservations::new(reservation_bytes).map_err(Error::Reservation)?;

    Ok(Self {
      header,
      structure,
      strings,
      reservations,
    })
  }

  /// Returns the root node of the devicetree.
  #[must_use]
  pub fn root(&self) -> Node<'a> {
    self.structure.root(self.strings)
  }

  /// Returns an iterator over physical memory ranges described by `/memory`
  /// nodes.
  ///
  /// Ranges from multiple `/memory` nodes and multiple address-size pairs
  /// within their `reg` properties are exposed through one iterator.
  ///
  /// Memory reservations are not excluded from the returned ranges.
  #[must_use]
  pub fn memory_ranges(&self) -> MemoryRanges<'a> {
    let root = self.root();
    MemoryRanges::new(&root)
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::{error::Error, header::HeaderError, reader::ReadError, structure::StructureError};

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
  const DEVICE_TYPE_NAME_OFFSET: u32 = 0;
  const REG_NAME_OFFSET: u32 = 12;

  const FDT_BEGIN_NODE: u32 = 0x1;
  const FDT_PROP: u32 = 0x3;
  const FDT_END_NODE: u32 = 0x2;
  const FDT_END: u32 = 0x9;

  #[repr(align(8))]
  struct Aligned<const N: usize>([u8; N]);

  fn set_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
  }

  // TODO: Centralize the test DTB layout offsets if this grows further.
  fn minimal_blob() -> Aligned<164> {
    let mut bytes = [0; 164];

    set_u32(&mut bytes, MAGIC, 0xd00d_feed);
    set_u32(&mut bytes, TOTAL_SIZE, 164);
    set_u32(&mut bytes, STRUCTURE_OFFSET, 56);
    set_u32(&mut bytes, STRINGS_OFFSET, 148);
    set_u32(&mut bytes, RESERVATION_OFFSET, 40);
    set_u32(&mut bytes, VERSION, 17);
    set_u32(&mut bytes, LAST_COMP_VERSION, 16);
    set_u32(&mut bytes, BOOT_CPUID_PHYS, 0);
    set_u32(&mut bytes, STRINGS_SIZE, 16);
    set_u32(&mut bytes, STRUCTURE_SIZE, 92);

    // /
    set_u32(&mut bytes, 56, FDT_BEGIN_NODE);

    // /cpus
    set_u32(&mut bytes, 64, FDT_BEGIN_NODE);
    bytes[68..73].copy_from_slice(b"cpus\0");
    set_u32(&mut bytes, 76, FDT_END_NODE);

    // /memory
    set_u32(&mut bytes, 80, FDT_BEGIN_NODE);
    bytes[84..91].copy_from_slice(b"memory\0");

    // device_type = "memory"
    set_u32(&mut bytes, 92, FDT_PROP);
    set_u32(&mut bytes, 96, 7);
    set_u32(&mut bytes, 100, DEVICE_TYPE_NAME_OFFSET);
    bytes[104..111].copy_from_slice(b"memory\0");

    // reg = <0x0 0x0 0x1000>
    //
    // Root defaults:
    // #address-cells = 2
    // #size-cells = 1
    set_u32(&mut bytes, 112, FDT_PROP);
    set_u32(&mut bytes, 116, 12);
    set_u32(&mut bytes, 120, REG_NAME_OFFSET);
    set_u32(&mut bytes, 124, 0);
    set_u32(&mut bytes, 128, 0);
    set_u32(&mut bytes, 132, 0x1000);

    // /memory
    set_u32(&mut bytes, 136, FDT_END_NODE);

    // /
    set_u32(&mut bytes, 140, FDT_END_NODE);

    set_u32(&mut bytes, 144, FDT_END);

    // Strings block
    bytes[148..160].copy_from_slice(b"device_type\0");
    bytes[160..164].copy_from_slice(b"reg\0");

    Aligned(bytes)
  }

  #[test]
  fn valid_blob_is_constructed() {
    let blob = minimal_blob();

    // SAFETY: `blob` contains the complete initialized DTB and remains alive
    // for the returned `Fdt`.
    let fdt = unsafe { Fdt::from_ptr(blob.0.as_ptr()) }.expect("valid DTB should parse");

    assert_eq!(fdt.header.total_size(), 164);
    assert_eq!(fdt.header.structure_range(), 56..148);
    assert_eq!(fdt.header.strings_range(), 148..164);
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

    set_u32(&mut blob.0, 56, FDT_END);

    let error = unsafe { Fdt::from_ptr(blob.0.as_ptr()) }.unwrap_err();

    assert_eq!(
      error,
      Error::Structure(StructureError::ExpectedRootNode {
        offset: 0,
        found: FDT_END,
      })
    );
  }

  #[test]
  fn reservation_error_is_propagated() {
    let mut blob = minimal_blob();

    blob.0[40..48].copy_from_slice(&1_u64.to_be_bytes());
    blob.0[48..56].copy_from_slice(&1_u64.to_be_bytes());

    assert_eq!(
      unsafe { Fdt::from_ptr(blob.0.as_ptr()) }.unwrap_err(),
      Error::Reservation(ReadError::Truncated {
        offset: 16,
        requested: 8,
        remaining: 0,
      })
    );
  }

  #[test]
  fn root_returns_root_node() {
    let bytes = minimal_blob();

    let fdt = unsafe { Fdt::from_ptr(bytes.0.as_ptr()).expect("valid DTB should parse") };

    assert_eq!(fdt.root().name(), b"");
  }

  #[test]
  fn memory_ranges_are_exposed() {
    let blob = minimal_blob();

    let fdt = unsafe { Fdt::from_ptr(blob.0.as_ptr()) }.expect("valid DTB should parse");

    let mut ranges = fdt.memory_ranges();

    let range = ranges.next().expect("memory range should exist");

    assert_eq!(range.address(), 0);
    assert_eq!(range.size(), 0x1000);
    assert!(ranges.next().is_none());
  }
}
