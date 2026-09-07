//! Shared validation and interpretation of `reg` entries as address-size
//! ranges.
//!
//! This module provides the representation common to semantic consumers of
//! `reg` address-size pairs without assigning meaning to the represented
//! ranges.

use super::addressing::RegLayout;

/// Width of one 32-bit Devicetree cell in bytes.
const CELL_SIZE: usize = size_of::<u32>();

/// Numeric radix used when combining consecutive 32-bit Devicetree cells.
const CELL_RADIX: u64 = 0x1_0000_0000;

/// An error encountered while validating a `reg` property as address-size
/// ranges representable by this implementation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RegRangeError {
  /// The property contains no address-size entries.
  Empty,

  /// An encoded address cannot be represented as a `u64`.
  AddressDoesNotFitU64,

  /// An encoded size cannot be represented as a `u64`.
  SizeDoesNotFitU64,

  /// An entry encodes a size of zero bytes.
  ZeroSize,
}

/// One validated address-size range decoded from a `reg` property.
///
/// The type represents only the numeric address and size. It does not describe
/// what the range represents semantically.
#[derive(Clone, Copy)]
pub(super) struct RegRange {
  /// Decoded start address of the range.
  address: u64,

  /// Decoded size of the range in bytes.
  size: u64,
}

impl RegRange {
  /// Returns the decoded start address of the range.
  #[must_use]
  pub(super) const fn address(self) -> u64 {
    self.address
  }

  /// Returns the decoded size of the range in bytes.
  #[must_use]
  pub(super) const fn size(self) -> u64 {
    self.size
  }
}

/// Iterator over validated address-size entries encoded by a `reg` property.
pub(super) struct RegRanges<'a> {
  /// Portion of the validated `reg` property that has not yet been consumed.
  bytes: &'a [u8],

  /// Byte layout used to split each remaining entry into address and size
  /// fields.
  layout: RegLayout,
}

/// Validates `reg` entries as non-empty, non-zero address-size ranges whose
/// values are representable by `u64`.
///
/// `bytes` must already consist of complete entries according to `layout`.
///
/// # Errors
///
/// Returns [`RegRangeError::Empty`] if no entries are present.
///
/// Returns [`RegRangeError::AddressDoesNotFitU64`] if an address exceeds
/// [`u64::MAX`].
///
/// Returns [`RegRangeError::SizeDoesNotFitU64`] if a size exceeds
/// [`u64::MAX`].
///
/// Returns [`RegRangeError::ZeroSize`] if an entry has size zero.
pub(super) fn validate(bytes: &[u8], layout: RegLayout) -> Result<(), RegRangeError> {
  if bytes.is_empty() {
    return Err(RegRangeError::Empty);
  }

  for entry in bytes.chunks_exact(layout.entry_size()) {
    let (address, size) = entry.split_at(layout.address_size());

    if decode_cells_u64(address).is_none() {
      return Err(RegRangeError::AddressDoesNotFitU64);
    }

    let Some(size) = decode_cells_u64(size) else {
      return Err(RegRangeError::SizeDoesNotFitU64);
    };

    if size == 0 {
      return Err(RegRangeError::ZeroSize);
    }
  }

  Ok(())
}

/// Decodes a sequence of big-endian 32-bit Devicetree cells into a `u64`.
///
/// Cells are interpreted as base [`CELL_RADIX`] digits with the first cell
/// containing the most significant part of the value.
///
/// Returns `None` if `bytes` does not contain a whole number of 32-bit cells or
/// if the represented integer exceeds [`u64::MAX`].
fn decode_cells_u64(bytes: &[u8]) -> Option<u64> {
  let (cells, remainder) = bytes.as_chunks::<CELL_SIZE>();

  if !remainder.is_empty() {
    return None;
  }

  let mut value = 0_u64;

  for cell in cells {
    let cell = u64::from(u32::from_be_bytes(*cell));

    value = value.checked_mul(CELL_RADIX)?;
    value = value.checked_add(cell)?;
  }

  Some(value)
}

/// Decodes cells previously established as representable by
/// [`decode_cells_u64`].
///
/// # Panics
///
/// Panics if the established representability invariant does not hold.
#[expect(
  clippy::expect_used,
  reason = "semantic range validation guarantees range values fit in u64"
)]
fn decode_validated_cells_u64(bytes: &[u8]) -> u64 {
  decode_cells_u64(bytes).expect("validated range value must fit in u64")
}

impl<'a> RegRanges<'a> {
  /// Creates an iterator over entries previously accepted by [`validate`].
  ///
  /// `bytes` must have been validated using the supplied `layout`.
  pub(super) const fn new(bytes: &'a [u8], layout: RegLayout) -> Self {
    Self { bytes, layout }
  }
}

impl Iterator for RegRanges<'_> {
  type Item = RegRange;

  fn next(&mut self) -> Option<Self::Item> {
    if self.bytes.is_empty() {
      return None;
    }

    let (entry, remaining) = self.bytes.split_at(self.layout.entry_size());

    self.bytes = remaining;

    let (address, size) = entry.split_at(self.layout.address_size());

    Some(RegRange {
      address: decode_validated_cells_u64(address),
      size: decode_validated_cells_u64(size),
    })
  }
}
