//! Parsing and validation of FDT node names.

/// Errors produced while validating an FDT node name.
#[derive(Debug, PartialEq, Eq)]
pub enum NodeNameError {
  /// The node-name component is empty or exceeds the maximum permitted length.
  InvalidLength {
    /// Length of the node-name component in bytes.
    length: usize,
  },

  /// The node-name component does not begin with an alphabetic character.
  InvalidFirstCharacter {
    /// Invalid first byte.
    byte: u8,
  },

  /// The node-name component contains a character not permitted by the
  /// Devicetree Specification.
  InvalidCharacter {
    /// Byte index of the invalid character within the node-name component.
    index: usize,

    /// Invalid byte.
    byte: u8,
  },

  /// A unit-address separator is present without a following unit address.
  EmptyUnitAddress,

  /// The unit-address component contains a character not permitted by the
  /// Devicetree Specification.
  InvalidUnitAddressCharacter {
    /// Byte index of the invalid character within the unit-address component.
    index: usize,

    /// Invalid byte.
    byte: u8,
  },
}

/// Returns whether a provided `byte` is permitted in an FDT node-name or
/// unit-address component after the node name's first character.
const fn is_valid_character(byte: u8) -> bool {
  byte.is_ascii_alphanumeric() || matches!(byte, b',' | b'.' | b'_' | b'+' | b'-')
}

/// Returns the node-name component of a full node name.
pub(super) fn component(name: &[u8]) -> &[u8] {
  name.split(|&byte| byte == b'@').next().unwrap_or(name)
}

/// Returns the unit-address component of a full node name.
pub(super) fn unit_address(name: &[u8]) -> Option<&[u8]> {
  name.splitn(2, |&byte| byte == b'@').nth(1)
}

/// Validates a non-root FDT node name.
///
/// A node name may consist of a node-name component alone or a node-name
/// component followed by `@` and a unit-address component.
///
/// The node-name component:
///
/// - Contains between 1 and 31 bytes.
/// - Begins with an ASCII alphabetic character.
/// - Contains only characters permitted for node names by the specification.
///
/// If a unit address is present, it must be nonempty and contain only
/// characters permitted by the same node-name character set.
///
/// This function validates only the textual form of the unit address. It does
/// not interpret the address or compare it with any property value.
///
/// # Errors
///
/// Returns:
///
/// - [`NodeNameError::InvalidLength`] if the node-name component is empty or
///   exceeds 31 bytes.
/// - [`NodeNameError::InvalidFirstCharacter`] if the node-name component does
///   not begin with an ASCII alphabetic character.
/// - [`NodeNameError::InvalidCharacter`] if the node-name component contains an
///   invalid byte.
/// - [`NodeNameError::EmptyUnitAddress`] if `@` is present without a following
///   unit address.
/// - [`NodeNameError::InvalidUnitAddressCharacter`] if the unit-address
///   component contains an invalid byte.
pub(super) fn validate(name: &[u8]) -> Result<(), NodeNameError> {
  let node_name = component(name);
  let unit_address = unit_address(name);

  let Some(&first_byte) = node_name.first() else {
    return Err(NodeNameError::InvalidLength { length: 0 });
  };

  if node_name.len() > 31 {
    return Err(NodeNameError::InvalidLength {
      length: node_name.len(),
    });
  }

  if !first_byte.is_ascii_alphabetic() {
    return Err(NodeNameError::InvalidFirstCharacter { byte: first_byte });
  }

  if let Some((index, byte)) = node_name
    .iter()
    .copied()
    .enumerate()
    .find(|&(_, byte)| !is_valid_character(byte))
  {
    return Err(NodeNameError::InvalidCharacter { index, byte });
  }

  if let Some(unit_address) = unit_address {
    if unit_address.is_empty() {
      return Err(NodeNameError::EmptyUnitAddress);
    }

    if let Some((index, byte)) = unit_address
      .iter()
      .copied()
      .enumerate()
      .find(|&(_, byte)| !is_valid_character(byte))
    {
      return Err(NodeNameError::InvalidUnitAddressCharacter { index, byte });
    }
  }

  Ok(())
}

#[cfg(test)]
mod test {
  use super::*;

  #[test]
  fn valid_node_names_are_accepted() {
    for name in [
      b"a".as_slice(),
      b"soc".as_slice(),
      b"serial@1000".as_slice(),
      b"vendor,device".as_slice(),
      b"a.b_c+d-0".as_slice(),
    ] {
      assert_eq!(validate(name), Ok(()));
    }
  }

  #[test]
  fn empty_node_name_is_rejected() {
    assert_eq!(
      validate(b""),
      Err(NodeNameError::InvalidLength { length: 0 })
    );
  }

  #[test]
  fn node_name_longer_than_31_bytes_is_rejected() {
    let name = [b'a'; 32];

    assert_eq!(
      validate(&name),
      Err(NodeNameError::InvalidLength { length: 32 })
    );
  }

  #[test]
  fn node_name_must_begin_with_alphabetic_character() {
    assert_eq!(
      validate(b"1node"),
      Err(NodeNameError::InvalidFirstCharacter { byte: b'1' })
    );
  }

  #[test]
  fn invalid_node_name_character_is_rejected() {
    assert_eq!(
      validate(b"no?de"),
      Err(NodeNameError::InvalidCharacter {
        index: 2,
        byte: b'?',
      })
    );
  }

  #[test]
  fn empty_unit_address_is_rejected() {
    assert_eq!(validate(b"serial@"), Err(NodeNameError::EmptyUnitAddress));
  }

  #[test]
  fn invalid_unit_address_character_is_rejected() {
    assert_eq!(
      validate(b"serial@10?0"),
      Err(NodeNameError::InvalidUnitAddressCharacter {
        index: 2,
        byte: b'?',
      })
    );
  }
}
