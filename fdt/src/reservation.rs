//! Parsing and validation of the FDT memory reservation block.
//!
//! Implementation based on [Devicetree Specification v0.4](https://www.devicetree.org/).

use core::fmt;

use crate::reader::{ReadError, Reader};

/// An error encountered while parsing or validating the FDT memory
/// reservation block.
#[derive(Debug, PartialEq, Eq)]
pub enum ReservationError {
  /// The reservation block ended before a complete entry could be read.
  Read(ReadError),

  /// A reservation's end address cannot be represented as a `u64`.
  RangeEndOverflow {
    /// Start address of the reservation.
    address: u64,

    /// Size of the reservation in bytes.
    size: u64,
  },

  /// Two reservation-block regions overlap.
  Overlap {
    /// Start address of the first reservation.
    first_address: u64,

    /// Size of the first reservation.
    first_size: u64,

    /// Start address of the second reservation.
    second_address: u64,

    /// Size of the second reservation.
    second_size: u64,
  },
}

impl From<ReadError> for ReservationError {
  fn from(error: ReadError) -> Self {
    Self::Read(error)
  }
}

/// Width of one encoded reservation-block value in bytes.
const RESERVATION_VALUE_SIZE: usize = size_of::<u64>();

/// A validated view of an FDT memory reservation block.
///
/// The retained byte slice consists of complete `(address, size)` entries and
/// ends with the required `(0, 0)` terminating entry.
///
/// Construction guarantees that every reservation has a representable end
/// address and that non-empty reserved regions do not overlap.
///
/// These guarantees do not imply that the described regions correspond to
/// usable or otherwise valid physical memory.
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

impl fmt::Debug for MemoryReservation {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.debug_struct("MemoryReservation")
      .field("address", &format_args!("{:#x}", self.address))
      .field("size", &format_args!("{:#x}", self.size))
      .finish()
  }
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
#[derive(Clone)]
pub struct MemoryReservations<'a> {
  /// Portion of the validated reservation block that has not yet been consumed.
  bytes: &'a [u8],
}

/// Returns the exclusive end address of a memory reservation.
///
/// # Errors
///
/// Returns [`ReservationError::RangeEndOverflow`] if `address + size` cannot
/// be represented as a `u64`.
fn reservation_end(address: u64, size: u64) -> Result<u64, ReservationError> {
  address
    .checked_add(size)
    .ok_or(ReservationError::RangeEndOverflow { address, size })
}

impl<'a> Reservations<'a> {
  /// Validates an FDT memory reservation block and constructs a view over its
  /// encoded entries.
  ///
  /// `bytes` must begin at the first reservation entry and extend far enough
  /// to contain the required `(0, 0)` terminating entry. Bytes following the
  /// terminator are ignored and are not retained by the returned view.
  ///
  /// Construction establishes that every reservation has a representable end
  /// address and that reservation regions do not overlap.
  ///
  /// # Errors
  ///
  /// Returns [`ReservationError::Read`] if the block ends before a complete
  /// terminating entry can be read.
  ///
  /// Returns [`ReservationError::RangeEndOverflow`] if the end address of a
  /// reservation cannot be represented as a `u64`.
  ///
  /// Returns [`ReservationError::Overlap`] if two reserved regions overlap.
  pub(super) fn new(bytes: &'a [u8]) -> Result<Self, ReservationError> {
    let mut reader = Reader::new(bytes);

    let end = loop {
      let address = reader.read_u64()?;
      let size = reader.read_u64()?;

      if address == 0 && size == 0 {
        break reader.position();
      }

      reservation_end(address, size)?;
    };

    #[expect(
      clippy::indexing_slicing,
      reason = "`Reader` guarantees its position never exceeds the length of its backing slice"
    )]
    let bytes = &bytes[..end];

    let reservations = Self { bytes };

    reservations.validate_no_overlaps()?;

    Ok(reservations)
  }

  /// Returns an iterator over the memory reservations retained by this block.
  pub(super) const fn iter(&self) -> MemoryReservations<'a> {
    MemoryReservations { bytes: self.bytes }
  }

  /// Validates that memory reservation block entries do not overlap.
  ///
  /// Each reservation is compared with every reservation that follows it in the
  /// block. Zero-sized reservations do not describe any addresses and are
  /// therefore ignored for overlap purposes.
  ///
  /// # Errors
  ///
  /// Returns [`ReservationError::RangeEndOverflow`] if a reservation's end
  /// address cannot be represented.
  ///
  /// Returns [`ReservationError::Overlap`] if any two non-empty reserved
  /// physical memory regions overlap.
  fn validate_no_overlaps(&self) -> Result<(), ReservationError> {
    let mut remaining = self.iter();

    while let Some(first) = remaining.next() {
      if first.size() == 0 {
        continue;
      }

      let first_end = reservation_end(first.address(), first.size())?;

      for second in remaining.clone() {
        if second.size() == 0 {
          continue;
        }

        let second_end = reservation_end(second.address(), second.size())?;

        let overlaps = first.address() < second_end && second.address() < first_end;

        if overlaps {
          return Err(ReservationError::Overlap {
            first_address: first.address(),
            first_size: first.size(),
            second_address: second.address(),
            second_size: second.size(),
          });
        }
      }
    }

    Ok(())
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
  fn memory_reservation_debug_uses_hex() {
    let reservation = MemoryReservation {
      address: 0x8000_0000,
      size: 0x80000,
    };

    assert_eq!(
      std::format!("{reservation:?}"),
      "MemoryReservation { address: 0x80000000, size: 0x80000 }"
    );
  }

  #[test]
  fn missing_size_is_rejected() {
    let bytes = 0x1234_u64.to_be_bytes();

    assert_eq!(
      Reservations::new(&bytes).unwrap_err(),
      ReservationError::Read(ReadError::Truncated {
        offset: 8,
        requested: 8,
        remaining: 0,
      })
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

  #[test]
  fn reservation_range_end_overflow_is_rejected() {
    let mut bytes = Vec::new();

    push_reservation(&mut bytes, u64::MAX, 1);
    push_reservation(&mut bytes, 0, 0);

    assert_eq!(
      Reservations::new(&bytes).unwrap_err(),
      ReservationError::RangeEndOverflow {
        address: u64::MAX,
        size: 1,
      }
    );
  }

  #[test]
  fn overlapping_reservations_are_rejected() {
    let mut bytes = Vec::new();

    push_reservation(&mut bytes, 0x1000, 0x2000);
    push_reservation(&mut bytes, 0x2000, 0x2000);
    push_reservation(&mut bytes, 0, 0);

    assert_eq!(
      Reservations::new(&bytes).unwrap_err(),
      ReservationError::Overlap {
        first_address: 0x1000,
        first_size: 0x2000,
        second_address: 0x2000,
        second_size: 0x2000,
      }
    );
  }

  #[test]
  fn adjacent_reservations_do_not_overlap() {
    let mut bytes = Vec::new();

    push_reservation(&mut bytes, 0x1000, 0x1000);
    push_reservation(&mut bytes, 0x2000, 0x1000);
    push_reservation(&mut bytes, 0, 0);

    assert!(Reservations::new(&bytes).is_ok());
  }

  #[test]
  fn zero_sized_reservation_does_not_overlap() {
    let mut bytes = Vec::new();

    // Empty range located inside the following non-empty range.
    push_reservation(&mut bytes, 0x1800, 0);
    push_reservation(&mut bytes, 0x1000, 0x1000);
    push_reservation(&mut bytes, 0, 0);

    let reservations = Reservations::new(&bytes).unwrap();

    let mut reservations = reservations.iter();

    let first = reservations.next().unwrap();
    assert_eq!(first.address(), 0x1800);
    assert_eq!(first.size(), 0);

    let second = reservations.next().unwrap();
    assert_eq!(second.address(), 0x1000);
    assert_eq!(second.size(), 0x1000);

    assert!(reservations.next().is_none());
  }
}
