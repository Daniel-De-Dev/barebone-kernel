//! Errors produced while parsing or validating an FDT.

/// An error encountered while parsing or validating an FDT.
// TODO: Eventually separate to nested error enums to better organize them example:
// pub enum Error {
//   Header(HeaderError),
//   Structure(StructureError),
//   Strings(StringError),
//   Reservation(ReservationError),
// }
// INFO: pub is placed temporary to let kernel access these directly
#[derive(Debug, PartialEq)]
pub enum Error {
  /// An arithmetic operation overflowed while calculating an offset or size.
  IntegerOverflow,

  /// The FDT header contains an invalid magic value.
  InvalidMagic {
    /// Magic value found in the header.
    found: u32,
  },

  /// The total size declared by the FDT header is smaller than the header
  /// itself.
  TotalSizeTooSmall {
    /// Total FDT size declared by the header.
    total_size: u32,
  },

  /// The FDT version is not supported by this parser.
  UnsupportedVersion {
    /// FDT format version declared by the header.
    version: u32,

    /// Earliest FDT version with which the blob declares compatibility.
    last_compatible: u32,
  },

  /// The memory reservation block is not aligned to an 8-byte boundary.
  MisalignedReservationBlock {
    /// Byte offset of the memory reservation block.
    offset: u32,
  },

  /// The structure block is not aligned to a 4-byte boundary.
  MisalignedStructureBlock {
    /// Byte offset of the structure block.
    offset: u32,
  },

  /// A block offset points to an invalid location within the FDT.
  InvalidOffset,

  /// A block extends beyond the bounds declared by the FDT header.
  OutOfBounds,

  /// Two FDT blocks overlap in memory.
  BlocksOverlap,
}
