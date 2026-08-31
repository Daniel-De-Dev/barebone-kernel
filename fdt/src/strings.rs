//! Validation of property names in an FDT strings block.
//!
//! [`Strings`] provides a bounded view of a strings block without validating
//! its contents. Property-name offsets must be validated individually before
//! the referenced names are relied upon.
//!
//! Implementation based on [Devicetree Specification v0.4](https://www.devicetree.org/).

use core::fmt;

use crate::{helpers, reader::Reader};

/// Errors produced while validating a property name in the strings block.
#[derive(Debug, PartialEq, Eq)]
pub enum PropertyNameError {
  /// The property-name offset lies outside the strings block.
  OffsetOutOfBounds {
    /// Raw `nameoff` value from the property.
    offset: u32,

    /// Size of the strings block in bytes.
    strings_size: usize,
  },

  /// No NUL terminator follows the property-name offset before the end of the
  /// strings block.
  Unterminated {
    /// Raw `nameoff` value from the property.
    offset: u32,
  },

  /// The property name is empty or exceeds the maximum permitted length.
  InvalidLength {
    /// Raw `nameoff` value from the property.
    offset: u32,

    /// Length of the property name in bytes, excluding its NUL terminator.
    length: usize,
  },

  /// The property name contains a character not permitted by the Devicetree
  /// Specification.
  InvalidCharacter {
    /// Raw `nameoff` value from the property.
    offset: u32,

    /// Byte index of the invalid character relative to the beginning of the
    /// property name.
    index: usize,

    /// Invalid byte.
    byte: u8,
  },
}

/// A bounded view of an FDT strings block.
///
/// `Strings` does not imply that the entire block has been validated.
/// Property-name offsets must be validated individually through
/// [`Strings::validate_property_name`] before relying on the referenced name.
#[derive(Clone, Copy)]
pub(super) struct Strings<'a> {
  /// Raw bytes of the FDT strings block.
  bytes: &'a [u8],
}

impl fmt::Debug for Strings<'_> {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.debug_struct("Strings")
      .field("size", &self.bytes.len())
      .finish()
  }
}

impl<'a> Strings<'a> {
  /// Creates a view over an FDT strings block.
  ///
  /// This performs no validation of the block contents.
  pub(super) const fn new(bytes: &'a [u8]) -> Self {
    Self { bytes }
  }

  /// Validates the property name referenced by `raw_offset`.
  ///
  /// Returns the validated property-name bytes, excluding the terminating NUL.
  ///
  /// A valid property name:
  ///
  /// - Begins within the strings block.
  /// - Is terminated by a NUL byte.
  /// - Contains between 1 and 31 bytes.
  /// - Contains only characters permitted for property names by the Devicetree
  ///   Specification.
  ///
  /// The offset is not required to point immediately after another NUL byte.
  ///
  /// # Errors
  ///
  /// Returns:
  ///
  /// - [`PropertyNameError::OffsetOutOfBounds`] if `raw_offset` does not point
  ///   into the strings block.
  /// - [`PropertyNameError::Unterminated`] if no NUL terminator follows the
  ///   offset.
  /// - [`PropertyNameError::InvalidLength`] if the resulting name is empty or
  ///   exceeds 31 bytes.
  /// - [`PropertyNameError::InvalidCharacter`] if the name contains a byte
  ///   outside the permitted character set.
  pub(super) fn validate_property_name(
    &self,
    raw_offset: u32,
  ) -> Result<&'a [u8], PropertyNameError> {
    let offset = helpers::usize_from_u32(raw_offset);

    let Some(bytes) = self.bytes.get(offset..).filter(|bytes| !bytes.is_empty()) else {
      return Err(PropertyNameError::OffsetOutOfBounds {
        offset: raw_offset,
        strings_size: self.bytes.len(),
      });
    };

    let mut reader = Reader::new(bytes);

    let name = reader
      .read_nul_terminated()
      .map_err(|_| PropertyNameError::Unterminated { offset: raw_offset })?;

    if !(1..=31).contains(&name.len()) {
      return Err(PropertyNameError::InvalidLength {
        offset: raw_offset,
        length: name.len(),
      });
    }

    if let Some((index, byte)) = name.iter().copied().enumerate().find(|&(_, byte)| {
      !byte.is_ascii_alphanumeric()
        && !matches!(byte, b',' | b'.' | b'_' | b'+' | b'?' | b'#' | b'-')
    }) {
      return Err(PropertyNameError::InvalidCharacter {
        offset: raw_offset,
        index,
        byte,
      });
    }

    Ok(name)
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  extern crate std;

  #[test]
  fn debug_reports_strings_size() {
    let strings = Strings::new(b"compatible\0reg\0");

    assert_eq!(std::format!("{strings:?}"), "Strings { size: 15 }");
  }

  #[test]
  fn valid_property_name_is_accepted() {
    let strings = Strings::new(b"compatible\0");

    assert!(strings.validate_property_name(0).is_ok());
  }

  #[test]
  fn property_name_at_nonzero_offset_is_accepted() {
    let strings = Strings::new(b"unused\0compatible\0");

    assert!(strings.validate_property_name(7).is_ok());
  }

  #[test]
  fn property_name_at_suffix_offset_is_accepted() {
    let strings = Strings::new(b"vendor,compatible\0");

    assert!(strings.validate_property_name(7).is_ok());
  }

  #[test]
  fn all_allowed_property_name_characters_are_accepted() {
    let strings = Strings::new(b"aZ0,._+?#-\0");

    assert!(strings.validate_property_name(0).is_ok());
  }

  #[test]
  fn minimum_property_name_length_is_accepted() {
    let strings = Strings::new(b"a\0");

    assert!(strings.validate_property_name(0).is_ok());
  }

  #[test]
  fn maximum_property_name_length_is_accepted() {
    let mut data = [b'a'; 32];
    data[31] = 0;

    let strings = Strings::new(&data);

    assert!(strings.validate_property_name(0).is_ok());
  }

  #[test]
  fn empty_property_name_is_rejected() {
    let strings = Strings::new(b"\0");

    assert_eq!(
      strings.validate_property_name(0),
      Err(PropertyNameError::InvalidLength {
        offset: 0,
        length: 0,
      })
    );
  }

  #[test]
  fn property_name_longer_than_31_bytes_is_rejected() {
    let mut data = [b'a'; 33];
    data[32] = 0;

    let strings = Strings::new(&data);

    assert_eq!(
      strings.validate_property_name(0),
      Err(PropertyNameError::InvalidLength {
        offset: 0,
        length: 32,
      })
    );
  }

  #[test]
  fn unterminated_property_name_is_rejected() {
    let strings = Strings::new(b"compatible");

    assert_eq!(
      strings.validate_property_name(0),
      Err(PropertyNameError::Unterminated { offset: 0 })
    );
  }

  #[test]
  fn offset_at_end_of_strings_block_is_rejected() {
    let data = b"name\0";
    let strings = Strings::new(data);

    assert_eq!(
      strings.validate_property_name(data.len() as u32),
      Err(PropertyNameError::OffsetOutOfBounds {
        offset: data.len() as u32,
        strings_size: data.len(),
      })
    );
  }

  #[test]
  fn offset_beyond_strings_block_is_rejected() {
    let data = b"name\0";
    let strings = Strings::new(data);

    assert_eq!(
      strings.validate_property_name(6),
      Err(PropertyNameError::OffsetOutOfBounds {
        offset: 6,
        strings_size: data.len(),
      })
    );
  }

  #[test]
  fn invalid_property_name_character_is_rejected() {
    let strings = Strings::new(b"unused\0comp@tible\0");

    assert_eq!(
      strings.validate_property_name(7),
      Err(PropertyNameError::InvalidCharacter {
        offset: 7,
        index: 4,
        byte: b'@',
      })
    );
  }
}
