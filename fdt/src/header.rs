//! Parsing and validation of the FDT header.
//!
//! An FDT begins with a fixed-size, big-endian header describing the total blob
//! size, format version, boot CPU identifier, and the locations of its
//! variable-sized blocks.
//!
//! This module decodes that header and validates every layout property that can
//! be determined from the header alone.
//!
//! The header does not encode the size of the memory reservation block.
//! Its starting offset can be validated from the header fields, but its
//! complete extent cannot be determined from the header alone.
//!
//! Implementation based on [Devicetree Specification v0.4](https://www.devicetree.org/).

use core::ops::Range;

use crate::helpers;

/// Size of an FDT header in bytes.
pub(super) const HEADER_SIZE: usize = 40;

/// Magic value identifying an FDT.
const FDT_MAGIC: u32 = 0xd00d_feed;

/// FDT format version supported by this parser.
const SUPPORTED_VERSION: u32 = 17;

/// Required backwards-compatible version for [`SUPPORTED_VERSION`].
const EXPECTED_LAST_COMPATIBLE_VERSION: u32 = 16;

/// Identifies one of the fixed-width 32-bit fields in an FDT header.
///
/// Each variant corresponds to a four-byte field defined by the FDT header
/// layout. [`HeaderField::offset`] returns the byte offset at which that field
/// begins relative to the start of the header.
///
/// Because this enum can only represent known header fields, every returned
/// offset refers to a four-byte range fully contained within [`HEADER_SIZE`].
#[derive(Clone, Copy)]
enum HeaderField {
  /// FDT magic value.
  Magic,

  /// Total size of the DTB in bytes.
  TotalSize,

  /// Byte offset of the structure block.
  StructureOffset,

  /// Byte offset of the strings block.
  StringsOffset,

  /// Byte offset of the memory reservation block.
  ReservationOffset,

  /// FDT format version.
  Version,

  /// Earliest FDT version with which this blob is backwards-compatible.
  LastCompatibleVersion,

  /// Physical identifier of the boot CPU.
  BootCpuIdPhys,

  /// Size of the strings block in bytes.
  StringsSize,

  /// Size of the structure block in bytes.
  StructureSize,
}

impl HeaderField {
  /// Returns the byte offset of this field within the fixed-size FDT header.
  const fn offset(self) -> usize {
    match self {
      Self::Magic => 0,
      Self::TotalSize => 4,
      Self::StructureOffset => 8,
      Self::StringsOffset => 12,
      Self::ReservationOffset => 16,
      Self::Version => 20,
      Self::LastCompatibleVersion => 24,
      Self::BootCpuIdPhys => 28,
      Self::StringsSize => 32,
      Self::StructureSize => 36,
    }
  }
}

/// Identifies a variable-sized block referenced by the FDT header.
#[derive(Debug, PartialEq, Eq)]
pub enum BlockKind {
  /// Memory reservation block.
  MemoryReservation,

  /// Structure block.
  Structure,

  /// Strings block.
  Strings,
}

/// An error encountered while decoding or validating an FDT header.
#[derive(Debug, PartialEq, Eq)]
pub enum HeaderError {
  /// The header does not contain the FDT magic value.
  InvalidMagic {
    /// Magic value read from the header.
    found: u32,
  },

  /// The declared blob size is too small to contain the header.
  TotalSizeTooSmall {
    /// Declared total blob size, in bytes.
    total_size: usize,

    /// Minimum permitted blob size, in bytes.
    minimum: usize,
  },

  /// The header declares an unsupported FDT version pair.
  UnsupportedVersion {
    /// FDT format version.
    version: u32,

    /// Lowest format version with which the blob declares compatibility.
    last_compatible: u32,
  },

  /// A block offset does not satisfy its required alignment.
  MisalignedBlock {
    /// Block containing the invalid offset.
    block: BlockKind,

    /// Byte offset from the beginning of the FDT.
    offset: usize,

    /// Required byte alignment.
    required_alignment: usize,
  },

  /// A block begins within the fixed-size FDT header.
  BlockOffsetInsideHeader {
    /// Block containing the invalid offset.
    block: BlockKind,

    /// Byte offset from the beginning of the FDT.
    offset: usize,

    /// Size of the fixed FDT header, in bytes.
    header_size: usize,
  },

  /// A block start lies beyond the declared end of the FDT blob.
  BlockOffsetOutOfBounds {
    /// Block containing the invalid offset.
    block: BlockKind,

    /// Byte offset from the beginning of the FDT.
    offset: usize,

    /// Declared total blob size, in bytes.
    total_size: usize,
  },

  /// A known-size block does not fit within the declared FDT blob.
  BlockOutOfBounds {
    /// Block whose range is invalid.
    block: BlockKind,

    /// Byte offset at which the block begins.
    offset: usize,

    /// Declared block size, in bytes.
    size: usize,

    /// Declared total blob size, in bytes.
    total_size: usize,
  },

  /// Two known block ranges overlap.
  BlocksOverlap {
    /// First overlapping block.
    first: BlockKind,

    /// Second overlapping block.
    second: BlockKind,
  },

  /// A block begins inside another known block.
  BlockOffsetInsideBlock {
    /// Block whose start lies inside another block.
    block: BlockKind,

    /// Block containing the invalid start offset.
    containing: BlockKind,

    /// Byte offset of the block start.
    offset: usize,
  },
}

/// Validated information decoded from an FDT header.
///
/// Instances can only be constructed through [`Header::new`]. The stored
/// structure and strings ranges have been checked to lie within
/// [`Header::total_size`] and not overlap each other.
#[derive(Debug, Clone)]
pub(super) struct Header {
  /// Declared total size of the FDT blob, in bytes.
  total_size: usize,

  /// Validated byte range occupied by the structure block.
  structure_range: Range<usize>,

  /// Validated byte range occupied by the strings block.
  strings_range: Range<usize>,

  /// Byte offset at which the memory reservation block begins.
  ///
  /// The offset is aligned, lies within the DTB, does not point inside the
  /// header, structure block, or strings block. The complete reservation-block
  /// extent is not known from the header alone.
  off_mem_rsvmap: usize,

  /// FDT format version decoded from the header.
  version: u32,

  /// Lowest FDT format version with which the blob declares compatibility.
  last_comp_version: u32,

  /// Physical identifier of the CPU on which the system booted.
  boot_cpuid_phys: u32,
}

/// Reads a big-endian `u32` from a field within an FDT header.
///
/// [`HeaderField`] guarantees that the selected four-byte field is fully
/// contained within the fixed-size header, so this operation is infallible.
const fn read_be_u32(bytes: &[u8; HEADER_SIZE], field: HeaderField) -> u32 {
  let offset = field.offset();

  #[expect(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "HeaderField can only represent validated four-byte FDT header fields"
  )]
  u32::from_be_bytes([
    bytes[offset],
    bytes[offset + 1],
    bytes[offset + 2],
    bytes[offset + 3],
  ])
}

/// Reads a big-endian `u32` header field and converts it to `usize`.
///
/// The conversion is lossless on every target supported by this crate.
const fn read_be_u32_as_usize(bytes: &[u8; HEADER_SIZE], field: HeaderField) -> usize {
  helpers::usize_from_u32(read_be_u32(bytes, field))
}

/// Constructs a validated range for a known-size FDT block.
///
/// The returned range is guaranteed to end at or before `total_size`.
/// Arithmetic overflow or an end beyond `total_size` is rejected.
///
/// # Errors
///
/// Returns [`HeaderError::BlockOutOfBounds`] if `offset` lies beyond
/// `total_size` or if `size` exceeds the number of bytes remaining from
/// `offset`.
const fn block_range(
  block: BlockKind,
  offset: usize,
  size: usize,
  total_size: usize,
) -> Result<Range<usize>, HeaderError> {
  let Some(end) = offset.checked_add(size) else {
    return Err(HeaderError::BlockOutOfBounds {
      block,
      offset,
      size,
      total_size,
    });
  };

  if end > total_size {
    return Err(HeaderError::BlockOutOfBounds {
      block,
      offset,
      size,
      total_size,
    });
  }

  Ok(offset..end)
}

impl Header {
  /// Decodes and validates a fixed-size FDT header.
  ///
  /// The header fields are interpreted as big-endian values. Validation
  /// establishes all layout properties that can be determined without
  /// inspecting the contents of the variable-sized blocks.
  ///
  /// On success:
  ///
  /// - `total_size` is at least [`HEADER_SIZE`].
  /// - The FDT version pair is supported.
  /// - The memory reservation offset is 8-byte aligned.
  /// - The structure offset is 4-byte aligned.
  /// - All block offsets begin at or after the end of the header.
  /// - The memory reservation offset does not exceed `total_size`.
  /// - The structure and strings ranges lie entirely within `total_size`.
  /// - The structure and strings ranges do not overlap.
  /// - The memory reservation block does not begin inside the structure or
  ///   strings block.
  ///
  /// # Errors
  ///
  /// Returns:
  ///
  /// - [`HeaderError::InvalidMagic`] if the magic value is incorrect.
  /// - [`HeaderError::TotalSizeTooSmall`] if the declared blob cannot contain
  ///   the fixed-size header.
  /// - [`HeaderError::UnsupportedVersion`] if the version pair is unsupported.
  /// - [`HeaderError::MisalignedBlock`] if a block offset does not satisfy its
  ///   required alignment.
  /// - [`HeaderError::BlockOffsetInsideHeader`] if a block begins within the
  ///   fixed-size header.
  /// - [`HeaderError::BlockOffsetOutOfBounds`] if the memory reservation offset
  ///   lies beyond the declared blob.
  /// - [`HeaderError::BlockOutOfBounds`] if the structure or strings block does
  ///   not fit within the declared blob.
  /// - [`HeaderError::BlocksOverlap`] if the structure and strings ranges
  ///   overlap.
  /// - [`HeaderError::BlockOffsetInsideBlock`] if the memory reservation block
  ///   begins inside the structure or strings block.
  pub(super) fn new(bytes: &[u8; HEADER_SIZE]) -> Result<Self, HeaderError> {
    let magic = read_be_u32(bytes, HeaderField::Magic);

    if magic != FDT_MAGIC {
      return Err(HeaderError::InvalidMagic { found: magic });
    }

    let total_size = read_be_u32_as_usize(bytes, HeaderField::TotalSize);
    let off_dt_struct = read_be_u32_as_usize(bytes, HeaderField::StructureOffset);
    let off_dt_strings = read_be_u32_as_usize(bytes, HeaderField::StringsOffset);
    let off_mem_rsvmap = read_be_u32_as_usize(bytes, HeaderField::ReservationOffset);

    let version = read_be_u32(bytes, HeaderField::Version);
    let last_comp_version = read_be_u32(bytes, HeaderField::LastCompatibleVersion);
    let boot_cpuid_phys = read_be_u32(bytes, HeaderField::BootCpuIdPhys);

    let size_dt_strings = read_be_u32_as_usize(bytes, HeaderField::StringsSize);
    let size_dt_struct = read_be_u32_as_usize(bytes, HeaderField::StructureSize);

    if total_size < HEADER_SIZE {
      return Err(HeaderError::TotalSizeTooSmall {
        total_size,
        minimum: HEADER_SIZE,
      });
    }

    if version != SUPPORTED_VERSION || last_comp_version != EXPECTED_LAST_COMPATIBLE_VERSION {
      return Err(HeaderError::UnsupportedVersion {
        version,
        last_compatible: last_comp_version,
      });
    }

    if !off_mem_rsvmap.is_multiple_of(8) {
      return Err(HeaderError::MisalignedBlock {
        block: BlockKind::MemoryReservation,
        offset: off_mem_rsvmap,
        required_alignment: 8,
      });
    }

    if !off_dt_struct.is_multiple_of(4) {
      return Err(HeaderError::MisalignedBlock {
        block: BlockKind::Structure,
        offset: off_dt_struct,
        required_alignment: 4,
      });
    }

    for (block, offset) in [
      (BlockKind::MemoryReservation, off_mem_rsvmap),
      (BlockKind::Structure, off_dt_struct),
      (BlockKind::Strings, off_dt_strings),
    ] {
      if offset < HEADER_SIZE {
        return Err(HeaderError::BlockOffsetInsideHeader {
          block,
          offset,
          header_size: HEADER_SIZE,
        });
      }
    }

    if off_mem_rsvmap > total_size {
      return Err(HeaderError::BlockOffsetOutOfBounds {
        block: BlockKind::MemoryReservation,
        offset: off_mem_rsvmap,
        total_size,
      });
    }

    let structure_range = block_range(
      BlockKind::Structure,
      off_dt_struct,
      size_dt_struct,
      total_size,
    )?;

    let strings_range = block_range(
      BlockKind::Strings,
      off_dt_strings,
      size_dt_strings,
      total_size,
    )?;

    if structure_range.start < strings_range.end && strings_range.start < structure_range.end {
      return Err(HeaderError::BlocksOverlap {
        first: BlockKind::Structure,
        second: BlockKind::Strings,
      });
    }

    for (containing, range) in [
      (BlockKind::Structure, &structure_range),
      (BlockKind::Strings, &strings_range),
    ] {
      if range.contains(&off_mem_rsvmap) {
        return Err(HeaderError::BlockOffsetInsideBlock {
          block: BlockKind::MemoryReservation,
          containing,
          offset: off_mem_rsvmap,
        });
      }
    }

    Ok(Self {
      total_size,
      structure_range,
      strings_range,
      off_mem_rsvmap,
      version,
      last_comp_version,
      boot_cpuid_phys,
    })
  }

  /// Returns the declared total size of the FDT blob, in bytes.
  pub(super) const fn total_size(&self) -> usize {
    self.total_size
  }

  /// Returns the validated starting offset of the memory reservation block.
  ///
  /// The offset is 8-byte aligned, lies within the DTB, and does not point
  /// inside the header, structure block, or strings block.
  pub(super) const fn reservation_offset(&self) -> usize {
    self.off_mem_rsvmap
  }

  /// Returns the validated byte range occupied by the structure block.
  ///
  /// The returned range lies entirely within [`Header::total_size`].
  pub(super) const fn structure_range(&self) -> Range<usize> {
    self.structure_range.start..self.structure_range.end
  }

  /// Returns the validated byte range occupied by the strings block.
  ///
  /// The returned range lies entirely within [`Header::total_size`].
  pub(super) const fn strings_range(&self) -> Range<usize> {
    self.strings_range.start..self.strings_range.end
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  // Helper to generate a known-good FDT header.
  fn valid_header_bytes() -> [u8; HEADER_SIZE] {
    let mut bytes = [0; HEADER_SIZE];

    set_u32(&mut bytes, HeaderField::Magic, FDT_MAGIC);
    set_u32(&mut bytes, HeaderField::TotalSize, 1024);
    set_u32(&mut bytes, HeaderField::StructureOffset, 64);
    set_u32(&mut bytes, HeaderField::StringsOffset, 512);
    set_u32(&mut bytes, HeaderField::ReservationOffset, 40);
    set_u32(&mut bytes, HeaderField::Version, 17);
    set_u32(&mut bytes, HeaderField::LastCompatibleVersion, 16);
    set_u32(&mut bytes, HeaderField::BootCpuIdPhys, 0x1234_5678);
    set_u32(&mut bytes, HeaderField::StringsSize, 100);
    set_u32(&mut bytes, HeaderField::StructureSize, 200);

    bytes
  }

  // Helper to write a specific u32 into an FDT header field.
  fn set_u32(bytes: &mut [u8; HEADER_SIZE], field: HeaderField, value: u32) {
    let offset = field.offset();

    bytes[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
  }

  #[test]
  fn valid_header_is_parsed() {
    let bytes = valid_header_bytes();
    let header = Header::new(&bytes).expect("valid header should parse");

    assert_eq!(header.total_size, 1024);
    assert_eq!(header.structure_range, 64..264);
    assert_eq!(header.strings_range, 512..612);
    assert_eq!(header.off_mem_rsvmap, 40);
    assert_eq!(header.version, 17);
    assert_eq!(header.last_comp_version, 16);
    assert_eq!(header.boot_cpuid_phys, 0x1234_5678);

    assert_eq!(header.total_size(), 1024);
    assert_eq!(header.structure_range(), 64..264);
    assert_eq!(header.strings_range(), 512..612);
  }

  #[test]
  fn invalid_magic_is_rejected() {
    let mut bytes = valid_header_bytes();

    set_u32(&mut bytes, HeaderField::Magic, 0xBAD_CAFE);

    assert_eq!(
      Header::new(&bytes).unwrap_err(),
      HeaderError::InvalidMagic { found: 0xBAD_CAFE }
    );
  }

  #[test]
  fn total_size_smaller_than_header_is_rejected() {
    let mut bytes = valid_header_bytes();

    set_u32(&mut bytes, HeaderField::TotalSize, (HEADER_SIZE - 1) as u32);

    assert_eq!(
      Header::new(&bytes).unwrap_err(),
      HeaderError::TotalSizeTooSmall {
        total_size: HEADER_SIZE - 1,
        minimum: HEADER_SIZE,
      }
    );
  }

  #[test]
  fn version_below_supported_version_is_rejected() {
    let mut bytes = valid_header_bytes();

    set_u32(&mut bytes, HeaderField::Version, 15);

    assert_eq!(
      Header::new(&bytes).unwrap_err(),
      HeaderError::UnsupportedVersion {
        version: 15,
        last_compatible: 16,
      }
    );
  }

  #[test]
  fn unexpected_last_compatible_version_is_rejected() {
    let mut bytes = valid_header_bytes();

    set_u32(&mut bytes, HeaderField::LastCompatibleVersion, 15);

    assert_eq!(
      Header::new(&bytes).unwrap_err(),
      HeaderError::UnsupportedVersion {
        version: 17,
        last_compatible: 15,
      }
    );
  }

  #[test]
  fn misaligned_reservation_offset_is_rejected() {
    let mut bytes = valid_header_bytes();

    set_u32(&mut bytes, HeaderField::ReservationOffset, 41);

    assert_eq!(
      Header::new(&bytes).unwrap_err(),
      HeaderError::MisalignedBlock {
        block: BlockKind::MemoryReservation,
        offset: 41,
        required_alignment: 8,
      }
    );
  }

  #[test]
  fn misaligned_structure_offset_is_rejected() {
    let mut bytes = valid_header_bytes();

    set_u32(&mut bytes, HeaderField::StructureOffset, 42);

    assert_eq!(
      Header::new(&bytes).unwrap_err(),
      HeaderError::MisalignedBlock {
        block: BlockKind::Structure,
        offset: 42,
        required_alignment: 4,
      }
    );
  }

  #[test]
  fn reservation_offset_inside_header_is_rejected() {
    let mut bytes = valid_header_bytes();

    set_u32(&mut bytes, HeaderField::ReservationOffset, 24);

    assert_eq!(
      Header::new(&bytes).unwrap_err(),
      HeaderError::BlockOffsetInsideHeader {
        block: BlockKind::MemoryReservation,
        offset: 24,
        header_size: 40,
      }
    );
  }

  #[test]
  fn structure_offset_inside_header_is_rejected() {
    let mut bytes = valid_header_bytes();

    set_u32(&mut bytes, HeaderField::StructureOffset, 36);

    assert_eq!(
      Header::new(&bytes).unwrap_err(),
      HeaderError::BlockOffsetInsideHeader {
        block: BlockKind::Structure,
        offset: 36,
        header_size: 40,
      }
    );
  }

  #[test]
  fn strings_offset_inside_header_is_rejected() {
    let mut bytes = valid_header_bytes();

    set_u32(&mut bytes, HeaderField::StringsOffset, 24);

    assert_eq!(
      Header::new(&bytes).unwrap_err(),
      HeaderError::BlockOffsetInsideHeader {
        block: BlockKind::Strings,
        offset: 24,
        header_size: 40,
      }
    );
  }

  #[test]
  fn reservation_offset_beyond_blob_is_rejected() {
    let mut bytes = valid_header_bytes();

    set_u32(&mut bytes, HeaderField::ReservationOffset, 1024 + 8);

    assert_eq!(
      Header::new(&bytes).unwrap_err(),
      HeaderError::BlockOffsetOutOfBounds {
        block: BlockKind::MemoryReservation,
        offset: 1032,
        total_size: 1024,
      }
    );
  }

  #[test]
  fn strings_block_out_of_bounds_is_rejected() {
    let mut bytes = valid_header_bytes();

    set_u32(&mut bytes, HeaderField::StringsSize, 1000);

    assert_eq!(
      Header::new(&bytes).unwrap_err(),
      HeaderError::BlockOutOfBounds {
        block: BlockKind::Strings,
        offset: 512,
        size: 1000,
        total_size: 1024,
      }
    );
  }

  #[test]
  fn structure_block_out_of_bounds_is_rejected() {
    let mut bytes = valid_header_bytes();

    set_u32(&mut bytes, HeaderField::StructureSize, 1000);

    assert_eq!(
      Header::new(&bytes).unwrap_err(),
      HeaderError::BlockOutOfBounds {
        block: BlockKind::Structure,
        offset: 64,
        size: 1000,
        total_size: 1024,
      }
    );
  }

  #[test]
  fn overlapping_structure_and_strings_blocks_are_rejected() {
    let mut bytes = valid_header_bytes();

    set_u32(&mut bytes, HeaderField::StructureOffset, 100);
    set_u32(&mut bytes, HeaderField::StructureSize, 100);
    set_u32(&mut bytes, HeaderField::StringsOffset, 150);

    assert_eq!(
      Header::new(&bytes).unwrap_err(),
      HeaderError::BlocksOverlap {
        first: BlockKind::Structure,
        second: BlockKind::Strings,
      }
    );
  }

  #[test]
  fn overflow_prone_structure_range_is_rejected() {
    let mut bytes = valid_header_bytes();

    set_u32(&mut bytes, HeaderField::TotalSize, u32::MAX);
    set_u32(&mut bytes, HeaderField::StructureOffset, u32::MAX - 3);
    set_u32(&mut bytes, HeaderField::StructureSize, 8);

    assert_eq!(
      Header::new(&bytes).unwrap_err(),
      HeaderError::BlockOutOfBounds {
        block: BlockKind::Structure,
        offset: (u32::MAX - 3) as usize,
        size: 8,
        total_size: u32::MAX as usize,
      }
    );
  }

  #[test]
  fn structure_offset_beyond_blob_is_rejected() {
    let mut bytes = valid_header_bytes();

    set_u32(&mut bytes, HeaderField::StructureOffset, 1028);
    set_u32(&mut bytes, HeaderField::StructureSize, 0);

    assert_eq!(
      Header::new(&bytes).unwrap_err(),
      HeaderError::BlockOutOfBounds {
        block: BlockKind::Structure,
        offset: 1028,
        size: 0,
        total_size: 1024,
      }
    );
  }

  #[test]
  fn block_range_arithmetic_overflow_is_rejected() {
    let offset = usize::MAX;
    let size = 1;
    let total_size = usize::MAX;

    assert_eq!(
      block_range(BlockKind::Structure, offset, size, total_size),
      Err(HeaderError::BlockOutOfBounds {
        block: BlockKind::Structure,
        offset,
        size,
        total_size,
      })
    );
  }

  #[test]
  fn reservation_offset_inside_known_block_is_rejected() {
    for (offset, containing) in [(72, BlockKind::Structure), (520, BlockKind::Strings)] {
      let mut bytes = valid_header_bytes();

      set_u32(&mut bytes, HeaderField::ReservationOffset, offset as u32);

      assert_eq!(
        Header::new(&bytes).unwrap_err(),
        HeaderError::BlockOffsetInsideBlock {
          block: BlockKind::MemoryReservation,
          containing,
          offset,
        }
      );
    }
  }
}
