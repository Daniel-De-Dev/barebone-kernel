//! Parsing and validation of the FDT memory reservation block.
//!
//! Implementation based on [Devicetree Specification v0.4](https://www.devicetree.org/).

use core::fmt;

use crate::reader::{ReadError, Reader};

/// Width of one encoded reservation-block value in bytes.
const RESERVATION_VALUE_SIZE: usize = size_of::<u64>();

/// A structurally validated view of an FDT memory reservation block.
///
/// The retained byte slice consists of complete 16-byte `(address, size)`
/// entries and ends with the required `(0, 0)` terminating entry.
///
/// Structural validity does not imply that the described physical address
/// ranges correspond to usable or otherwise valid memory regions.
pub(super) struct Reservations<'a> {
  /// Raw bytes of the validated memory reservation block, including its
  /// terminating `(0, 0)` entry.
  bytes: &'a [u8],
}

impl fmt::Debug for Reservations<'_> {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.debug_struct("Reservations")
      .field("size", &self.bytes.len())
      .finish()
  }
}

/// A physical memory region listed in the FDT memory reservation block.
///
/// The region is described exactly as encoded by the Devicetree: a physical
/// start address and a size in bytes.
#[derive(Clone, Copy)]
pub struct MemoryReservation {
  /// Physical start address of the reserved region.
  address: u64,

  /// Size of the reserved region in bytes.
  size: u64,
}

impl MemoryReservation {
  /// Returns the physical start address of the reserved region.
  #[must_use]
  pub const fn address(self) -> u64 {
    self.address
  }

  /// Returns the size of the reserved region in bytes.
  #[must_use]
  pub const fn size(self) -> u64 {
    self.size
  }
}

/// Iterator over physical memory regions listed in an FDT memory reservation
/// block.
///
/// The terminating `(0, 0)` entry is consumed but not yielded.
pub struct MemoryReservations<'a> {
  /// Portion of the validated reservation block that has not yet been consumed.
  bytes: &'a [u8],
}

impl<'a> Reservations<'a> {
  /// Validates an FDT memory reservation block and constructs a view over its
  /// encoded bytes.
  ///
  /// `bytes` must begin at the first reservation entry and extend far enough
  /// to contain the required `(0, 0)` terminating entry. Bytes following the
  /// terminator are ignored and are not retained by the returned view.
  ///
  /// Validation reads complete `(address, size)` entries until the terminating
  /// entry is encountered.
  ///
  /// # Errors
  ///
  /// Returns [`ReadError::Truncated`] if the slice ends before a complete
  /// terminating entry can be read.
  pub(super) fn new(bytes: &'a [u8]) -> Result<Self, ReadError> {
    let mut reader = Reader::new(bytes);

    loop {
      let address = reader.read_u64()?;
      let size = reader.read_u64()?;

      if address == 0 && size == 0 {
        let end = reader.position();

        #[expect(
          clippy::indexing_slicing,
          reason = "`Reader` guarantees its position never exceeds the length of its backing slice"
        )]
        let bytes = &bytes[..end];

        return Ok(Self { bytes });
      }
    }
  }

  /// Returns an iterator over the memory reservations retained by this block.
  pub(super) const fn iter(&self) -> MemoryReservations<'a> {
    MemoryReservations { bytes: self.bytes }
  }
}

impl Iterator for MemoryReservations<'_> {
  type Item = MemoryReservation;

  fn next(&mut self) -> Option<Self::Item> {
    let (address, remaining) = self.bytes.split_first_chunk::<RESERVATION_VALUE_SIZE>()?;
    let (size, remaining) = remaining.split_first_chunk::<RESERVATION_VALUE_SIZE>()?;

    self.bytes = remaining;

    let address = u64::from_be_bytes(*address);
    let size = u64::from_be_bytes(*size);

    if address == 0 && size == 0 {
      self.bytes = &[];
      return None;
    }

    Some(MemoryReservation { address, size })
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  extern crate std;
  use std::vec::Vec;

  fn push_reservation(bytes: &mut Vec<u8>, address: u64, size: u64) {
    bytes.extend_from_slice(&address.to_be_bytes());
    bytes.extend_from_slice(&size.to_be_bytes());
  }

  #[test]
  fn debug_reports_block_size() {
    let bytes = [0; 16];
    let reservations = Reservations::new(&bytes).unwrap();

    assert_eq!(
      std::format!("{reservations:?}"),
      "Reservations { size: 16 }"
    );
  }

  #[test]
  fn missing_size_is_rejected() {
    let bytes = 0x1234_u64.to_be_bytes();

    assert_eq!(
      Reservations::new(&bytes).unwrap_err(),
      ReadError::Truncated {
        offset: 8,
        requested: 8,
        remaining: 0,
      }
    );
  }

  #[test]
  fn memory_reservations_are_iterated_until_terminator() {
    let mut bytes = Vec::new();

    push_reservation(&mut bytes, 0x8000_0000, 0x1000);
    push_reservation(&mut bytes, 0x9000_0000, 0x20_0000);
    push_reservation(&mut bytes, 0, 0);

    // Must not become part of the retained reservation block.
    push_reservation(&mut bytes, 0xa000_0000, 0x1000);

    let reservations = Reservations::new(&bytes).unwrap();
    let mut reservations = reservations.iter();

    let first = reservations.next().unwrap();
    assert_eq!(first.address(), 0x8000_0000);
    assert_eq!(first.size(), 0x1000);

    let second = reservations.next().unwrap();
    assert_eq!(second.address(), 0x9000_0000);
    assert_eq!(second.size(), 0x20_0000);

    assert!(reservations.next().is_none());
    assert!(reservations.next().is_none());
  }
}
