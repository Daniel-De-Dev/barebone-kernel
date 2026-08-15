//! Errors produced while parsing a flattened devicetree.

/// Errors produced while parsing or validating a flattened devicetree.
// INFO: pub is placed temporary to let kernel access these directly
#[derive(Debug, PartialEq)]
pub enum Error {
  InvalidMagic { found: u32 },

  IntegerOverflow,

  UnexpectedEnd,

  HeaderTooSmall,

  TotalSizeTooSmall { total_size: u32 },
  UnsupportedVersion { version: u32, last_compatible: u32 },
  MisalignedReservationBlock { offset: u32 },
  MisalignedStructureBlock { offset: u32 },
  InvalidBlockOrder,
  InvalidOffset,
  OutOfBounds,
  BlocksOverlap,
}
