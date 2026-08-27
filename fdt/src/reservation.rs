//! Parsing and validation of the FDT memory reservation block.
//!
//! Implementation based on [Devicetree Specification v0.4](https://www.devicetree.org/).

use core::fmt;

use crate::reader::{ReadError, Reader};

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
}

#[cfg(test)]
mod tests {
  use super::*;

  extern crate std;

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
}
