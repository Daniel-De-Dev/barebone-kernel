//! Parsing and validation of the FDT header.
//!
//! Every FDT begins with a fixed-size header containing the information required
//! to locate and interpret the memory reservation, structure, and strings blocks
//! of the blob.
//!
//! This module validates the header before any of the variable-sized FDT blocks
//! are parsed.
//!
//! Validation of data contained within those blocks is left to their respective
//! parsers. Bounds and overlap validation for the memory reservation block is
//! deferred, as its size can only be determined while parsing it.
//!
//! Implementation based on [Devicetree Specification v0.4](https://www.devicetree.org/).

use crate::error::Error;

/// Size of an FDT header in bytes.
const HEADER_SIZE: u32 = 40;

/// Magic value identifying an FDT.
const FDT_MAGIC: u32 = 0xd00d_feed;

/// Minimum FDT format version supported by this parser.
const SUPPORTED_VERSION: u32 = 17;

/// Parsed data from an FDT header.
///
/// Construction is performed through [`Header::parse`], which validates the
/// header before returning a `Header`.
#[derive(Debug, Clone)]
// INFO: pub is placed temporary to let kernel access these directly
pub struct Header {
  /// Total size of the FDT blob, in bytes.
  total_size: u32,

  /// Byte offset from the beginning of the FDT header to the structure block.
  off_dt_struct: u32,

  /// Byte offset from the beginning of the FDT header to the strings block.
  off_dt_strings: u32,

  /// Byte offset from the beginning of the FDT header to the memory reservation
  /// block.
  off_mem_rsvmap: u32,

  /// Version of the FDT data structure.
  version: u32,

  /// Lowest FDT data structure version with which this blob is backwards
  /// compatible.
  last_comp_version: u32,

  /// Physical ID of the CPU on which the system booted.
  boot_cpuid_phys: u32,

  /// Size of the strings block, in bytes.
  size_dt_strings: u32,

  /// Size of the structure block, in bytes.
  size_dt_struct: u32,
}

impl Header {
  /// Parses and validates an FDT header.
  ///
  /// `bytes` must contain exactly the 40-byte FDT header. Each field is decoded
  /// from its big-endian representation and the resulting layout is checked
  /// before a [`Header`] is returned.
  ///
  /// Validation currently ensures that:
  ///
  /// - The FDT magic value is valid.
  /// - The declared total size can contain the header.
  /// - The FDT version and backwards-compatibility version are supported.
  /// - The memory reservation block is 8-byte aligned.
  /// - The structure block is 4-byte aligned.
  /// - Block offsets do not point inside the header.
  /// - Computing known block boundaries does not overflow.
  /// - The structure and strings blocks fit within the declared total size.
  /// - And the structure and strings blocks do not overlap.
  ///
  /// The size of the memory reservation block is not encoded directly in the
  /// header. Its complete bounds therefore cannot be validated here and must be
  /// checked when that block is parsed.
  ///
  /// # Errors
  ///
  /// Returns:
  ///
  /// - [`Error::InvalidMagic`] if the FDT magic value is incorrect.
  /// - [`Error::TotalSizeTooSmall`] if the FDT is smaller than the
  ///   header itself.
  /// - [`Error::UnsupportedVersion`] if the FDT version or declared
  ///   backwards-compatible version is unsupported.
  /// - [`Error::MisalignedReservationBlock`] if the memory reservation block is
  ///   not 8-byte aligned.
  /// - [`Error::MisalignedStructureBlock`] if the structure block is not
  ///   4-byte aligned.
  /// - [`Error::IntegerOverflow`] if calculating the end of a block overflows.
  /// - [`Error::InvalidOffset`] if a block begins inside the header.
  /// - [`Error::OutOfBounds`] if a known block extends beyond the declared FDT
  ///   size.
  /// - [`Error::BlocksOverlap`] if the structure and strings blocks overlap.
  // INFO: pub is placed temporary to let kernel access these directly
  pub fn parse(bytes: &[u8; 40]) -> Result<Self, Error> {
    let magic = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);

    if magic != FDT_MAGIC {
      return Err(Error::InvalidMagic { found: magic });
    }

    let total_size = u32::from_be_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    let off_dt_struct = u32::from_be_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]);
    let off_dt_strings = u32::from_be_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]);
    let off_mem_rsvmap = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
    let version = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
    let last_comp_version = u32::from_be_bytes([bytes[24], bytes[25], bytes[26], bytes[27]]);
    let boot_cpuid_phys = u32::from_be_bytes([bytes[28], bytes[29], bytes[30], bytes[31]]);
    let size_dt_strings = u32::from_be_bytes([bytes[32], bytes[33], bytes[34], bytes[35]]);
    let size_dt_struct = u32::from_be_bytes([bytes[36], bytes[37], bytes[38], bytes[39]]);

    if total_size < HEADER_SIZE {
      return Err(Error::TotalSizeTooSmall { total_size });
    }

    const EXPECTED_LAST_COMPATIBLE_VERSION: u32 = 16;

    if version < SUPPORTED_VERSION || last_comp_version != EXPECTED_LAST_COMPATIBLE_VERSION {
      return Err(Error::UnsupportedVersion {
        version,
        last_compatible: last_comp_version,
      });
    }

    if off_mem_rsvmap % 8 != 0 {
      return Err(Error::MisalignedReservationBlock {
        offset: off_mem_rsvmap,
      });
    }

    if off_dt_struct % 4 != 0 {
      return Err(Error::MisalignedStructureBlock {
        offset: off_dt_struct,
      });
    }

    let struct_end = off_dt_struct
      .checked_add(size_dt_struct)
      .ok_or(Error::IntegerOverflow)?;

    let strings_end = off_dt_strings
      .checked_add(size_dt_strings)
      .ok_or(Error::IntegerOverflow)?;

    // Validate that blocks start after header
    if off_mem_rsvmap < HEADER_SIZE || off_dt_struct < HEADER_SIZE || off_dt_strings < HEADER_SIZE {
      return Err(Error::InvalidOffset);
    }

    // Validate that known block sizes fit within declared total size
    // NOTE: memory block's size is unknown at this time and this needs to be validated later
    if struct_end > total_size || strings_end > total_size {
      return Err(Error::OutOfBounds);
    }

    // Validate that known blocks don't overlap
    // NOTE: memory block's size is unknown at this time and this needs to be validated later
    if (off_dt_struct < strings_end) && (struct_end > off_dt_strings) {
      return Err(Error::BlocksOverlap);
    }

    Ok(Self {
      total_size,
      off_dt_struct,
      off_dt_strings,
      off_mem_rsvmap,
      version,
      last_comp_version,
      boot_cpuid_phys,
      size_dt_strings,
      size_dt_struct,
    })
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::error::Error;

  // Helper to generate a known-good FDT header.
  fn valid_header_bytes() -> [u8; 40] {
    let mut bytes = [0u8; 40];

    bytes[0..4].copy_from_slice(&FDT_MAGIC.to_be_bytes());
    bytes[4..8].copy_from_slice(&1024u32.to_be_bytes()); // total_size
    bytes[8..12].copy_from_slice(&64u32.to_be_bytes()); // off_dt_struct
    bytes[12..16].copy_from_slice(&512u32.to_be_bytes()); // off_dt_strings
    bytes[16..20].copy_from_slice(&40u32.to_be_bytes()); // off_mem_rsvmap
    bytes[20..24].copy_from_slice(&17u32.to_be_bytes()); // version
    bytes[24..28].copy_from_slice(&16u32.to_be_bytes()); // last_comp_version
    bytes[28..32].copy_from_slice(&0u32.to_be_bytes()); // boot_cpuid_phys
    bytes[32..36].copy_from_slice(&100u32.to_be_bytes()); // size_dt_strings
    bytes[36..40].copy_from_slice(&200u32.to_be_bytes()); // size_dt_struct

    bytes
  }

  // Helper to write a specific u32 into a byte array at a given offset
  fn mutate_field(bytes: &mut [u8; 40], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
  }

  #[test]
  fn test_valid_header() {
    let bytes = valid_header_bytes();
    let header = Header::parse(&bytes).expect("Failed to parse valid header");

    assert_eq!(header.version, 17);
    assert_eq!(header.last_comp_version, 16);
    assert_eq!(header.total_size, 1024);
  }

  #[test]
  fn test_invalid_magic() {
    let mut bytes = valid_header_bytes();

    // Change Magic
    mutate_field(&mut bytes, 0, 0xBAD_CAFE);

    assert_eq!(
      Header::parse(&bytes).unwrap_err(),
      Error::InvalidMagic { found: 0xBAD_CAFE }
    );
  }

  #[test]
  fn test_small_size() {
    let mut bytes = valid_header_bytes();

    // Change size to less than 40
    mutate_field(&mut bytes, 4, 30);

    assert_eq!(
      Header::parse(&bytes).unwrap_err(),
      Error::TotalSizeTooSmall { total_size: 30 }
    );
  }

  #[test]
  fn test_unsupported_version() {
    let mut bytes = valid_header_bytes();

    // Set version to lower than 16
    mutate_field(&mut bytes, 20, 15);

    assert_eq!(
      Header::parse(&bytes).unwrap_err(),
      Error::UnsupportedVersion {
        version: 15,
        last_compatible: 16
      }
    );
  }

  #[test]
  fn test_invalid_last_comp_version() {
    let mut bytes = valid_header_bytes();

    // Set last_comp_version to less than 16
    mutate_field(&mut bytes, 24, 15);

    assert_eq!(
      Header::parse(&bytes).unwrap_err(),
      Error::UnsupportedVersion {
        version: 17,
        last_compatible: 15
      }
    );
  }

  #[test]
  fn test_misaligned_rsvmap() {
    let mut bytes = valid_header_bytes();

    // Set off_mem_rsvmap to a non 8 byte aligned address
    mutate_field(&mut bytes, 16, 41);

    assert_eq!(
      Header::parse(&bytes).unwrap_err(),
      Error::MisalignedReservationBlock { offset: 41 }
    );
  }

  #[test]
  fn test_misaligned_struct_block() {
    let mut bytes = valid_header_bytes();

    // Set off_dt_struct to a non 4 byte aligned address
    mutate_field(&mut bytes, 8, 41);

    assert_eq!(
      Header::parse(&bytes).unwrap_err(),
      Error::MisalignedStructureBlock { offset: 41 }
    );
  }

  #[test]
  fn test_integer_overflow_struct_end() {
    let mut bytes = valid_header_bytes();

    // (u32::MAX - 3) + 10 = overflow
    mutate_field(&mut bytes, 8, u32::MAX - 3);
    mutate_field(&mut bytes, 36, 10);

    assert_eq!(Header::parse(&bytes).unwrap_err(), Error::IntegerOverflow);
  }

  #[test]
  fn test_integer_overflow_strings_end() {
    let mut bytes = valid_header_bytes();

    // (u32::MAX - 10) + 20 = overflow
    mutate_field(&mut bytes, 12, u32::MAX - 10);
    mutate_field(&mut bytes, 32, 20);

    assert_eq!(Header::parse(&bytes).unwrap_err(), Error::IntegerOverflow);
  }

  #[test]
  fn test_block_ends_within_size() {
    let mut bytes = valid_header_bytes();

    mutate_field(&mut bytes, 36, 1000);
    mutate_field(&mut bytes, 32, 1000);

    assert_eq!(Header::parse(&bytes).unwrap_err(), Error::OutOfBounds);
  }

  #[test]
  fn test_invalid_block_overlap() {
    let mut bytes = valid_header_bytes();

    // Force Structure block to overlap Strings block
    mutate_field(&mut bytes, 8, 100); // off_dt_struct
    mutate_field(&mut bytes, 36, 100); // size_dt_struct (Ends at 200)
    mutate_field(&mut bytes, 12, 150); // off_dt_strings (Starts before Structure ends)

    assert_eq!(Header::parse(&bytes).unwrap_err(), Error::BlocksOverlap);
  }

  #[test]
  fn test_offsets_before_header_ends() {
    let mut bytes = valid_header_bytes();

    // set off_mem_rsvmap to starts inside header
    mutate_field(&mut bytes, 16, 24);

    assert_eq!(Header::parse(&bytes).unwrap_err(), Error::InvalidOffset);
  }
}
