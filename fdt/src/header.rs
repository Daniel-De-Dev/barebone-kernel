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
//! Only its starting offset can be bounded here; its complete extent and any
//! overlap involving that extent cannot be determined from the header alone.
//!
//! Implementation based on [Devicetree Specification v0.4](https://www.devicetree.org/).

use core::ops::Range;

/// Size of an FDT header in bytes.
pub(super) const HEADER_SIZE: usize = 40;

/// Magic value identifying an FDT.
const FDT_MAGIC: u32 = 0xd00d_feed;

/// FDT format version supported by this parser.
const SUPPORTED_VERSION: u32 = 17;

/// Required backwards-compatible version for [`SUPPORTED_VERSION`].
const EXPECTED_LAST_COMPATIBLE_VERSION: u32 = 16;

/// Byte offset of the `magic` field within the FDT header.
const MAGIC: usize = 0;

/// Byte offset of the `totalsize` field within the FDT header.
const TOTAL_SIZE: usize = 4;

/// Byte offset of the `off_dt_struct` field within the FDT header.
const STRUCTURE_OFFSET: usize = 8;

/// Byte offset of the `off_dt_strings` field within the FDT header.
const STRINGS_OFFSET: usize = 12;

/// Byte offset of the `off_mem_rsvmap` field within the FDT header.
const RESERVATION_OFFSET: usize = 16;

/// Byte offset of the `version` field within the FDT header.
const VERSION: usize = 20;

/// Byte offset of the `last_comp_version` field within the FDT header.
const LAST_COMP_VERSION: usize = 24;

/// Byte offset of the `boot_cpuid_phys` field within the FDT header.
const BOOT_CPUID_PHYS: usize = 28;

/// Byte offset of the `size_dt_strings` field within the FDT header.
const STRINGS_SIZE: usize = 32;

/// Byte offset of the `size_dt_struct` field within the FDT header.
const STRUCTURE_SIZE: usize = 36;

/// Identifies a variable-sized block referenced by the FDT header.
#[derive(Debug, PartialEq)]
pub enum BlockKind {
  /// Memory reservation block.
  MemoryReservation,

  /// Structure block.
  Structure,

  /// Strings block.
  Strings,
}

/// An error encountered while decoding or validating an FDT header.
#[derive(Debug, PartialEq)]
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
  /// The offset is aligned, does not point inside the header, and does not lie
  /// beyond `total_size`. The complete block extent is not known from the
  /// header.
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
/// `offset` is expected to identify the first byte of a four-byte field fully
/// contained within `bytes`.
fn read_be_u32(bytes: &[u8; HEADER_SIZE], offset: usize) -> u32 {
  u32::from_be_bytes([
    bytes[offset],
    bytes[offset + 1],
    bytes[offset + 2],
    bytes[offset + 3],
  ])
}

/// Constructs a validated range for a known-size FDT block.
///
/// The returned range is guaranteed to end at or before `total_size`. The
/// bounds check is performed before calculating `offset + size`, so the range
/// end cannot overflow.
///
/// # Errors
///
/// Returns [`HeaderError::BlockOutOfBounds`] if `offset` lies beyond
/// `total_size` or if `size` exceeds the number of bytes remaining from
/// `offset`.
fn block_range(
  block: BlockKind,
  offset: usize,
  size: usize,
  total_size: usize,
) -> Result<Range<usize>, HeaderError> {
  if offset > total_size || size > total_size - offset {
    return Err(HeaderError::BlockOutOfBounds {
      block,
      offset,
      size,
      total_size,
    });
  }

  Ok(offset..offset + size)
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
  pub(super) fn new(bytes: &[u8; HEADER_SIZE]) -> Result<Self, HeaderError> {
    let magic = read_be_u32(bytes, MAGIC);

    if magic != FDT_MAGIC {
      return Err(HeaderError::InvalidMagic { found: magic });
    }

    let total_size = read_be_u32(bytes, TOTAL_SIZE) as usize;
    let off_dt_struct = read_be_u32(bytes, STRUCTURE_OFFSET) as usize;
    let off_dt_strings = read_be_u32(bytes, STRINGS_OFFSET) as usize;
    let off_mem_rsvmap = read_be_u32(bytes, RESERVATION_OFFSET) as usize;

    let version = read_be_u32(bytes, VERSION);
    let last_comp_version = read_be_u32(bytes, LAST_COMP_VERSION);
    let boot_cpuid_phys = read_be_u32(bytes, BOOT_CPUID_PHYS);

    let size_dt_strings = read_be_u32(bytes, STRINGS_SIZE) as usize;
    let size_dt_struct = read_be_u32(bytes, STRUCTURE_SIZE) as usize;

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

    // The header provides the reservation block's start, but not its size.
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

    set_u32(&mut bytes, MAGIC, FDT_MAGIC);
    set_u32(&mut bytes, TOTAL_SIZE, 1024);
    set_u32(&mut bytes, STRUCTURE_OFFSET, 64);
    set_u32(&mut bytes, STRINGS_OFFSET, 512);
    set_u32(&mut bytes, RESERVATION_OFFSET, 40);
    set_u32(&mut bytes, VERSION, 17);
    set_u32(&mut bytes, LAST_COMP_VERSION, 16);
    set_u32(&mut bytes, BOOT_CPUID_PHYS, 0x1234_5678);
    set_u32(&mut bytes, STRINGS_SIZE, 100);
    set_u32(&mut bytes, STRUCTURE_SIZE, 200);

    bytes
  }

  // Helper to write a specific u32 into a byte array at a given offset.
  fn set_u32(bytes: &mut [u8; HEADER_SIZE], offset: usize, value: u32) {
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

    set_u32(&mut bytes, MAGIC, 0xBAD_CAFE);

    assert_eq!(
      Header::new(&bytes).unwrap_err(),
      HeaderError::InvalidMagic { found: 0xBAD_CAFE }
    );
  }

  #[test]
  fn total_size_smaller_than_header_is_rejected() {
    let mut bytes = valid_header_bytes();

    set_u32(&mut bytes, TOTAL_SIZE, (HEADER_SIZE - 1) as u32);

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

    set_u32(&mut bytes, VERSION, 15);

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

    set_u32(&mut bytes, LAST_COMP_VERSION, 15);

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

    set_u32(&mut bytes, RESERVATION_OFFSET, 41);

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

    set_u32(&mut bytes, STRUCTURE_OFFSET, 42);

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

    set_u32(&mut bytes, RESERVATION_OFFSET, 24);

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

    set_u32(&mut bytes, STRUCTURE_OFFSET, 36);

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

    set_u32(&mut bytes, STRINGS_OFFSET, 24);

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

    set_u32(&mut bytes, RESERVATION_OFFSET, 1024 + 8);

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

    set_u32(&mut bytes, STRINGS_SIZE, 1000);

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

    set_u32(&mut bytes, STRUCTURE_SIZE, 1000);

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

    set_u32(&mut bytes, STRUCTURE_OFFSET, 100);
    set_u32(&mut bytes, STRUCTURE_SIZE, 100);
    set_u32(&mut bytes, STRINGS_OFFSET, 150);

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

    set_u32(&mut bytes, TOTAL_SIZE, u32::MAX);
    set_u32(&mut bytes, STRUCTURE_OFFSET, u32::MAX - 3);
    set_u32(&mut bytes, STRUCTURE_SIZE, 8);

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

    set_u32(&mut bytes, STRUCTURE_OFFSET, 1028);
    set_u32(&mut bytes, STRUCTURE_SIZE, 0);

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
}
