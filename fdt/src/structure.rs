//! Parsing and validation of the FDT structure block.
//!
//! Validation covers token encoding and ordering, node nesting, node names,
//! referenced property names, property bounds, and termination of the structure
//! block. It does not validate higher-level devicetree semantics such as
//! required properties or relationships between property values.
//!
//! DTSpec requires alignment padding bytes to be zero. This parser does not
//! validate their contents and accepts nonzero padding for compatibility with
//! DTBs encountered in practice.
//!
//! Implementation based on [Devicetree Specification v0.4](https://www.devicetree.org/).

use core::fmt;

use crate::{
  reader::{ReadError, Reader},
  strings::{PropertyNameError, Strings},
};

/// Errors produced while validating an FDT node name.
#[derive(Debug, PartialEq)]
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

  /// The node-name component contains a character not permitted by the DTSpec.
  InvalidCharacter {
    /// Byte index of the invalid character within the node-name component.
    index: usize,

    /// Invalid byte.
    byte: u8,
  },

  /// A unit-address separator is present without a following unit address.
  EmptyUnitAddress,

  /// The unit-address component contains a character not permitted by the DTSpec.
  InvalidUnitAddressCharacter {
    /// Byte index of the invalid character within the unit-address component.
    index: usize,

    /// Invalid byte.
    byte: u8,
  },
}

/// An error encountered while validating an FDT structure block.
#[derive(Debug, PartialEq)]
pub enum StructureError {
  /// A bounded read of the structure block failed.
  Read(ReadError),

  /// A word in a token position does not encode a recognized FDT token.
  InvalidToken {
    /// Byte offset of the invalid token relative to the start of the structure
    /// block.
    offset: usize,

    /// Invalid token value.
    value: u32,
  },

  /// The first non-NOP token does not begin the root node.
  ExpectedRootNode {
    /// Byte offset of the unexpected token.
    offset: usize,

    /// Token value encountered instead of `FDT_BEGIN_NODE`.
    found: u32,
  },

  /// The root node contains a nonempty name.
  RootNameNotEmpty {
    /// Byte offset at which the root node name begins.
    offset: usize,

    /// First byte of the nonempty root name.
    first_byte: u8,
  },

  /// A node name has no NUL terminator before the end of the structure block.
  UnterminatedNodeName {
    /// Byte offset at which the node name begins.
    offset: usize,
  },

  /// A non-root node contains an invalid node name.
  InvalidNodeName {
    /// Byte offset at which the last token is.
    token_offset: usize,

    /// Byte offset at which the node name begins.
    name_offset: usize,

    /// The current node's depth.
    depth: usize,

    /// Reason the node name is invalid.
    source: NodeNameError,
  },

  /// A property appears after a child node within the same parent node.
  PropertyAfterChild {
    /// Byte offset of the property token.
    offset: usize,

    /// Tree depth at which the property was encountered.
    ///
    /// The root node has depth `1`.
    depth: usize,
  },

  /// A property references an invalid name in the strings block.
  InvalidPropertyName {
    /// Byte offset of the property token within the structure block.
    property_offset: usize,

    /// Raw strings-block offset encoded by the property.
    name_offset: u32,

    /// Reason the referenced property name is invalid.
    source: PropertyNameError,
  },

  /// The structure terminates before all open nodes have been closed.
  PrematureEnd {
    /// Byte offset of the premature `FDT_END` token.
    offset: usize,

    /// Number of nodes still open when `FDT_END` was encountered.
    depth: usize,
  },

  /// The root node has closed but the next non-NOP token is not `FDT_END`.
  ExpectedEnd {
    /// Byte offset of the unexpected token.
    offset: usize,

    /// Token value encountered instead of `FDT_END`.
    found: u32,
  },

  /// Bytes remain in the structure block after the final `FDT_END` token.
  TrailingData {
    /// Byte offset immediately following the `FDT_END` token.
    offset: usize,

    /// Number of trailing bytes.
    remaining: usize,
  },
}

impl From<ReadError> for StructureError {
  fn from(error: ReadError) -> Self {
    Self::Read(error)
  }
}

/// A validated view of an FDT structure block.
///
/// Instances can only be constructed through [`Structure::new`]. A `Structure`
/// represents one complete, structurally valid rooted tree and may be traversed
/// without repeating structural validation.
///
/// Structural validity does not imply that the represented devicetree satisfies
/// semantic requirements for any particular device, bus, or binding.
pub(super) struct Structure<'a> {
  /// Raw bytes of the validated structure block.
  bytes: &'a [u8],
}

impl fmt::Debug for Structure<'_> {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.debug_struct("Structure")
      .field("size", &self.bytes.len())
      .finish()
  }
}

/// Token values encoded in an FDT structure block.
#[repr(u32)]
#[derive(PartialEq)]
enum Token {
  /// Begins a node.
  BeginNode = 0x1,

  /// Ends the current node.
  EndNode = 0x2,

  /// Introduces a property.
  Property = 0x3,

  /// Carries no semantic meaning and may be ignored.
  Nop = 0x4,

  /// Terminates the structure block.
  End = 0x9,
}

impl Token {
  /// Decodes an FDT token value.
  ///
  /// # Errors
  ///
  /// Returns [`StructureError::InvalidToken`] if `value` does not encode a
  /// recognized structure-block token.
  fn parse(value: u32, offset: usize) -> Result<Self, StructureError> {
    match value {
      0x1 => Ok(Self::BeginNode),
      0x2 => Ok(Self::EndNode),
      0x3 => Ok(Self::Property),
      0x4 => Ok(Self::Nop),
      0x9 => Ok(Self::End),
      _ => Err(StructureError::InvalidToken { offset, value }),
    }
  }

  /// Returns the encoded 32-bit value of this token.
  const fn value(self) -> u32 {
    self as u32
  }
}

/// Tracks which token categories may still appear in the current node.
#[derive(PartialEq)]
enum NodePhase {
  /// Properties may appear because no child node has yet been encountered.
  Properties,

  /// At least one child has appeared, so further properties are forbidden.
  Children,
}

/// Returns whether a provided `byte` is permitted in an FDT node-name or
/// unit-address component after the node name's first character.
fn is_valid_name_character(byte: u8) -> bool {
  byte.is_ascii_alphanumeric() || matches!(byte, b',' | b'.' | b'_' | b'+' | b'-')
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
/// - Contains only characters permitted for node names by the DTSpec.
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
fn validate_node_name(name: &[u8]) -> Result<(), NodeNameError> {
  let (node_name, unit_address) = if let Some(at) = name.iter().position(|&byte| byte == b'@') {
    (&name[..at], Some(&name[at + 1..]))
  } else {
    (name, None)
  };

  if !(1..=31).contains(&node_name.len()) {
    return Err(NodeNameError::InvalidLength {
      length: node_name.len(),
    });
  }

  if !node_name[0].is_ascii_alphabetic() {
    return Err(NodeNameError::InvalidFirstCharacter { byte: node_name[0] });
  }

  if let Some(index) = node_name
    .iter()
    .position(|&byte| !is_valid_name_character(byte))
  {
    return Err(NodeNameError::InvalidCharacter {
      index,
      byte: node_name[index],
    });
  }

  if let Some(unit_address) = unit_address {
    if unit_address.is_empty() {
      return Err(NodeNameError::EmptyUnitAddress);
    }

    if let Some(index) = unit_address
      .iter()
      .position(|&byte| !is_valid_name_character(byte))
    {
      return Err(NodeNameError::InvalidUnitAddressCharacter {
        index,
        byte: unit_address[index],
      });
    }
  }

  Ok(())
}

/// Reads and decodes the next structure-block token.
///
/// Returns the byte offset at which the token begins together with its decoded
/// value.
///
/// # Errors
///
/// Returns [`StructureError::Read`] if a complete token cannot be read, or
/// [`StructureError::InvalidToken`] if the encoded value is not recognized.
fn read_token(reader: &mut Reader<'_>) -> Result<(usize, Token), StructureError> {
  let offset = reader.position();
  let value = reader.read_u32()?;
  let token = Token::parse(value, offset)?;

  Ok((offset, token))
}

/// Reads structure-block tokens until a token other than `FDT_NOP` is found.
///
/// Any number of consecutive `FDT_NOP` tokens are consumed and ignored. The
/// returned offset identifies the first byte of the returned non-NOP token.
///
/// # Errors
///
/// Returns [`StructureError::Read`] if the block ends before another
/// complete token is available, or [`StructureError::InvalidToken`] if an
/// unrecognized token value is encountered.
fn read_non_nop_token(reader: &mut Reader<'_>) -> Result<(usize, Token), StructureError> {
  loop {
    let (offset, token) = read_token(reader)?;

    if token != Token::Nop {
      return Ok((offset, token));
    }
  }
}

impl<'a> Structure<'a> {
  /// Validates an FDT structure block and constructs a view over its bytes.
  ///
  /// `bytes` must contain exactly one complete structure block. `strings`
  /// supplies the strings block against which property-name offsets are
  /// validated.
  ///
  /// On success, the structure block can be traversed without encountering
  /// malformed tokens, node boundaries, names, property bounds, or
  /// property-name references.
  ///
  /// Padding bytes are consumed to establish alignment, but their contents are
  /// intentionally not validated.
  ///
  /// Validation is structural rather than semantic. A successful result does
  /// not establish that required properties exist, that property values have
  /// binding-specific formats, or that relationships between nodes and
  /// properties are semantically correct.
  ///
  /// # Errors
  ///
  /// Returns:
  ///
  /// - [`StructureError::Read`] if any required token, field, value, or
  ///   alignment padding extends beyond the structure block.
  /// - [`StructureError::InvalidToken`] if an unrecognized token value is
  ///   encountered.
  /// - [`StructureError::ExpectedRootNode`] if the first non-NOP token does not
  ///   begin the root node.
  /// - [`StructureError::RootNameNotEmpty`] if the root node has a nonempty
  ///   name.
  /// - [`StructureError::UnterminatedNodeName`] if a child node name has no NUL
  ///   terminator.
  /// - [`StructureError::InvalidNodeName`] if a child node name has an invalid
  ///   form.
  /// - [`StructureError::PropertyAfterChild`] if a property occurs after a
  ///   child node in the same parent.
  /// - [`StructureError::InvalidPropertyName`] if a property references an
  ///   invalid name.
  /// - [`StructureError::PrematureEnd`] if `FDT_END` occurs while one or more
  ///   nodes remain open.
  /// - [`StructureError::ExpectedEnd`] if a non-NOP token other than `FDT_END`
  ///   follows the closed root node.
  /// - [`StructureError::TrailingData`] if bytes remain after `FDT_END`.
  pub(super) fn new(bytes: &'a [u8], strings: &Strings<'_>) -> Result<Self, StructureError> {
    let mut reader = Reader::new(bytes);

    // Arbitrary FDT_NOP tokens may precede the root node.
    let (root_token_offset, token) = read_non_nop_token(&mut reader)?;

    if token != Token::BeginNode {
      return Err(StructureError::ExpectedRootNode {
        offset: root_token_offset,
        found: token.value(),
      });
    }

    let mut depth = 1usize;
    let mut phase = NodePhase::Properties;

    // The root node has an empty name.
    let root_name_offset = reader.position();
    let first_byte = reader.read_bytes(1)?[0];

    if first_byte != 0 {
      return Err(StructureError::RootNameNotEmpty {
        offset: root_name_offset,
        first_byte,
      });
    }

    // Padding contents are ignored.
    reader.align_to_4()?;

    while depth > 0 {
      let (token_offset, token) = read_token(&mut reader)?;

      match token {
        Token::BeginNode => {
          let name_offset = reader.position();
          let name =
            reader
              .read_nul_terminated()
              .map_err(|_| StructureError::UnterminatedNodeName {
                offset: name_offset,
              })?;

          validate_node_name(name).map_err(|source| StructureError::InvalidNodeName {
            token_offset,
            name_offset,
            depth,
            source,
          })?;

          // Padding contents are ignored.
          reader.align_to_4()?;

          depth += 1;

          phase = NodePhase::Properties;
        }

        Token::EndNode => {
          depth -= 1;

          if depth > 0 {
            // Returning to a parent means at least one child has
            // already appeared, so further properties are forbidden.
            phase = NodePhase::Children;
          }
        }

        Token::Property => {
          if phase == NodePhase::Children {
            return Err(StructureError::PropertyAfterChild {
              offset: token_offset,
              depth,
            });
          }

          let length = reader.read_u32()?;
          let name_offset = reader.read_u32()?;

          strings
            .validate_property_name(name_offset)
            .map_err(|source| StructureError::InvalidPropertyName {
              property_offset: token_offset,
              name_offset,
              source,
            })?;

          reader.read_bytes(length as usize)?;

          // Padding contents are ignored.
          reader.align_to_4()?;
        }

        Token::Nop => {}

        Token::End => {
          return Err(StructureError::PrematureEnd {
            offset: token_offset,
            depth,
          });
        }
      }
    }

    // After the root closes, only FDT_NOP tokens followed by FDT_END
    // are permitted.
    let (end_token_offset, token) = read_non_nop_token(&mut reader)?;

    if token != Token::End {
      return Err(StructureError::ExpectedEnd {
        offset: end_token_offset,
        found: token.value(),
      });
    }

    if reader.remaining() != 0 {
      return Err(StructureError::TrailingData {
        offset: reader.position(),
        remaining: reader.remaining(),
      });
    }

    Ok(Self { bytes })
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  extern crate std;
  use std::vec::Vec;

  const COMPATIBLE_OFFSET: u32 = 0;
  const REG_OFFSET: u32 = 11;

  const STRINGS: &[u8] = b"compatible\0reg\0";

  fn push_u32(bytes: &mut Vec<u8>, value: u32) {
    bytes.extend_from_slice(&value.to_be_bytes());
  }

  fn push_token(bytes: &mut Vec<u8>, token: Token) -> usize {
    let offset = bytes.len();
    push_u32(bytes, token.value());
    offset
  }

  fn pad_to_4(bytes: &mut Vec<u8>) {
    while bytes.len() % 4 != 0 {
      bytes.push(0);
    }
  }

  fn push_begin_node(bytes: &mut Vec<u8>, name: &[u8]) -> usize {
    let offset = push_token(bytes, Token::BeginNode);

    bytes.extend_from_slice(name);
    bytes.push(0);
    pad_to_4(bytes);

    offset
  }

  fn push_end_node(bytes: &mut Vec<u8>) -> usize {
    push_token(bytes, Token::EndNode)
  }

  fn push_property(bytes: &mut Vec<u8>, name_offset: u32, value: &[u8]) -> usize {
    let offset = push_token(bytes, Token::Property);

    push_u32(bytes, value.len() as u32);
    push_u32(bytes, name_offset);
    bytes.extend_from_slice(value);
    pad_to_4(bytes);

    offset
  }

  fn push_nop(bytes: &mut Vec<u8>) -> usize {
    push_token(bytes, Token::Nop)
  }

  fn push_end(bytes: &mut Vec<u8>) -> usize {
    push_token(bytes, Token::End)
  }

  fn strings() -> Strings<'static> {
    Strings::new(STRINGS)
  }

  fn minimal_structure() -> Vec<u8> {
    let mut bytes = Vec::new();

    push_begin_node(&mut bytes, b"");
    push_end_node(&mut bytes);
    push_end(&mut bytes);

    bytes
  }

  #[test]
  fn debug_reports_structure_size() {
    let bytes = minimal_structure();
    let structure = Structure::new(&bytes, &strings()).expect("valid structure should parse");

    assert_eq!(std::format!("{structure:?}"), "Structure { size: 16 }");
  }

  // Node names
  #[test]
  fn valid_node_names_are_accepted() {
    for name in [
      b"a".as_slice(),
      b"soc".as_slice(),
      b"serial@1000".as_slice(),
      b"vendor,device".as_slice(),
      b"a.b_c+d-0".as_slice(),
    ] {
      assert_eq!(validate_node_name(name), Ok(()));
    }
  }

  #[test]
  fn empty_structure_block_is_rejected() {
    let bytes = [];

    assert_eq!(
      Structure::new(&bytes, &strings()).unwrap_err(),
      StructureError::Read(ReadError::Truncated {
        offset: 0,
        requested: 4,
        remaining: 0,
      })
    );
  }

  #[test]
  fn missing_root_node_name_is_rejected() {
    let mut bytes = Vec::new();

    push_token(&mut bytes, Token::BeginNode);

    assert_eq!(
      Structure::new(&bytes, &strings()).unwrap_err(),
      StructureError::Read(ReadError::Truncated {
        offset: 4,
        requested: 1,
        remaining: 0,
      })
    );
  }

  #[test]
  fn truncated_root_node_padding_is_rejected() {
    let mut bytes = Vec::new();

    push_token(&mut bytes, Token::BeginNode);
    bytes.push(0); // Empty root name, but missing 3 alignment bytes.

    assert_eq!(
      Structure::new(&bytes, &strings()).unwrap_err(),
      StructureError::Read(ReadError::Truncated {
        offset: 5,
        requested: 3,
        remaining: 0,
      })
    );
  }

  #[test]
  fn empty_node_name_is_rejected() {
    assert_eq!(
      validate_node_name(b""),
      Err(NodeNameError::InvalidLength { length: 0 })
    );
  }

  #[test]
  fn node_name_longer_than_31_bytes_is_rejected() {
    let name = [b'a'; 32];

    assert_eq!(
      validate_node_name(&name),
      Err(NodeNameError::InvalidLength { length: 32 })
    );
  }

  #[test]
  fn node_name_must_begin_with_alphabetic_character() {
    assert_eq!(
      validate_node_name(b"1node"),
      Err(NodeNameError::InvalidFirstCharacter { byte: b'1' })
    );
  }

  #[test]
  fn invalid_node_name_character_is_rejected() {
    assert_eq!(
      validate_node_name(b"no?de"),
      Err(NodeNameError::InvalidCharacter {
        index: 2,
        byte: b'?',
      })
    );
  }

  #[test]
  fn empty_unit_address_is_rejected() {
    assert_eq!(
      validate_node_name(b"serial@"),
      Err(NodeNameError::EmptyUnitAddress)
    );
  }

  #[test]
  fn invalid_unit_address_character_is_rejected() {
    assert_eq!(
      validate_node_name(b"serial@10?0"),
      Err(NodeNameError::InvalidUnitAddressCharacter {
        index: 2,
        byte: b'?',
      })
    );
  }

  // Valid structure blocks
  #[test]
  fn structure_with_properties_and_children_is_valid() {
    let mut bytes = Vec::new();

    push_begin_node(&mut bytes, b"");

    push_property(&mut bytes, COMPATIBLE_OFFSET, b"test,root");

    push_begin_node(&mut bytes, b"soc");
    push_property(&mut bytes, COMPATIBLE_OFFSET, b"simple-bus");
    push_end_node(&mut bytes);

    push_begin_node(&mut bytes, b"serial@1000");
    push_property(&mut bytes, REG_OFFSET, &[0x00, 0x00, 0x10, 0x00]);
    push_end_node(&mut bytes);

    push_end_node(&mut bytes);
    push_end(&mut bytes);

    assert!(Structure::new(&bytes, &strings()).is_ok());
  }

  #[test]
  fn properties_are_allowed_in_later_sibling_nodes() {
    let mut bytes = Vec::new();

    push_begin_node(&mut bytes, b"");

    push_begin_node(&mut bytes, b"first");
    push_end_node(&mut bytes);

    push_begin_node(&mut bytes, b"second");
    push_property(&mut bytes, COMPATIBLE_OFFSET, b"device");
    push_end_node(&mut bytes);

    push_end_node(&mut bytes);
    push_end(&mut bytes);

    assert!(Structure::new(&bytes, &strings()).is_ok());
  }

  #[test]
  fn nops_before_root_and_before_end_are_valid() {
    let mut bytes = Vec::new();

    push_nop(&mut bytes);
    push_nop(&mut bytes);

    push_begin_node(&mut bytes, b"");
    push_nop(&mut bytes);
    push_end_node(&mut bytes);

    push_nop(&mut bytes);
    push_nop(&mut bytes);

    push_end(&mut bytes);

    assert!(Structure::new(&bytes, &strings()).is_ok());
  }

  // Root validation
  #[test]
  fn non_begin_node_token_cannot_start_tree() {
    let mut bytes = Vec::new();

    push_nop(&mut bytes);
    let offset = push_token(&mut bytes, Token::Property);

    assert_eq!(
      Structure::new(&bytes, &strings()).unwrap_err(),
      StructureError::ExpectedRootNode {
        offset,
        found: Token::Property.value(),
      }
    );
  }

  #[test]
  fn root_name_must_be_empty() {
    let mut bytes = Vec::new();

    push_token(&mut bytes, Token::BeginNode);

    let name_offset = bytes.len();
    bytes.extend_from_slice(b"root\0");
    pad_to_4(&mut bytes);

    assert_eq!(
      Structure::new(&bytes, &strings()).unwrap_err(),
      StructureError::RootNameNotEmpty {
        offset: name_offset,
        first_byte: b'r',
      }
    );
  }

  // Node validation
  #[test]
  fn unterminated_child_node_name_is_rejected() {
    let mut bytes = Vec::new();

    push_begin_node(&mut bytes, b"");

    push_token(&mut bytes, Token::BeginNode);
    let name_offset = bytes.len();

    bytes.extend_from_slice(b"child");

    assert_eq!(
      Structure::new(&bytes, &strings()).unwrap_err(),
      StructureError::UnterminatedNodeName {
        offset: name_offset,
      }
    );
  }

  #[test]
  fn invalid_child_node_name_is_reported_with_offset() {
    let mut bytes = Vec::new();

    push_begin_node(&mut bytes, b"");

    let token_offset = push_begin_node(&mut bytes, b"1child");
    let name_offset = token_offset + 4;

    assert_eq!(
      Structure::new(&bytes, &strings()).unwrap_err(),
      StructureError::InvalidNodeName {
        token_offset,
        name_offset,
        depth: 1,
        source: NodeNameError::InvalidFirstCharacter { byte: b'1' },
      }
    );
  }

  // Property ordering and validation
  #[test]
  fn property_after_child_is_rejected() {
    let mut bytes = Vec::new();

    push_begin_node(&mut bytes, b"");

    push_begin_node(&mut bytes, b"child");
    push_end_node(&mut bytes);

    let property_offset = push_property(&mut bytes, COMPATIBLE_OFFSET, b"too-late");

    assert_eq!(
      Structure::new(&bytes, &strings()).unwrap_err(),
      StructureError::PropertyAfterChild {
        offset: property_offset,
        depth: 1,
      }
    );
  }

  #[test]
  fn missing_property_length_is_rejected() {
    let mut bytes = Vec::new();

    push_begin_node(&mut bytes, b"");
    push_token(&mut bytes, Token::Property);

    let expected_offset = bytes.len();

    assert_eq!(
      Structure::new(&bytes, &strings()).unwrap_err(),
      StructureError::Read(ReadError::Truncated {
        offset: expected_offset,
        requested: 4,
        remaining: 0,
      })
    );
  }

  #[test]
  fn missing_property_name_offset_is_rejected() {
    let mut bytes = Vec::new();

    push_begin_node(&mut bytes, b"");
    push_token(&mut bytes, Token::Property);
    push_u32(&mut bytes, 0);

    let expected_offset = bytes.len();

    assert_eq!(
      Structure::new(&bytes, &strings()).unwrap_err(),
      StructureError::Read(ReadError::Truncated {
        offset: expected_offset,
        requested: 4,
        remaining: 0,
      })
    );
  }

  #[test]
  fn truncated_property_padding_is_rejected() {
    let mut bytes = Vec::new();

    push_begin_node(&mut bytes, b"");

    push_token(&mut bytes, Token::Property);
    push_u32(&mut bytes, 1);
    push_u32(&mut bytes, COMPATIBLE_OFFSET);

    bytes.push(0xaa);

    let padding_offset = bytes.len();

    assert_eq!(
      Structure::new(&bytes, &strings()).unwrap_err(),
      StructureError::Read(ReadError::Truncated {
        offset: padding_offset,
        requested: 3,
        remaining: 0,
      })
    );
  }

  #[test]
  fn property_after_grandchild_is_rejected_in_parent() {
    let mut bytes = Vec::new();

    push_begin_node(&mut bytes, b"");

    push_begin_node(&mut bytes, b"parent");

    push_begin_node(&mut bytes, b"child");
    push_end_node(&mut bytes);

    let property_offset = push_property(&mut bytes, COMPATIBLE_OFFSET, b"too-late");

    assert_eq!(
      Structure::new(&bytes, &strings()).unwrap_err(),
      StructureError::PropertyAfterChild {
        offset: property_offset,
        depth: 2,
      }
    );
  }

  #[test]
  fn invalid_property_name_is_reported() {
    let strings = Strings::new(b"good\0bad@name\0");

    let mut bytes = Vec::new();

    push_begin_node(&mut bytes, b"");

    let property_offset = push_property(&mut bytes, 5, b"value");

    assert_eq!(
      Structure::new(&bytes, &strings).unwrap_err(),
      StructureError::InvalidPropertyName {
        property_offset,
        name_offset: 5,
        source: PropertyNameError::InvalidCharacter {
          offset: 5,
          index: 3,
          byte: b'@',
        },
      }
    );
  }

  #[test]
  fn truncated_property_value_is_rejected() {
    let mut bytes = Vec::new();

    push_begin_node(&mut bytes, b"");

    push_token(&mut bytes, Token::Property);
    push_u32(&mut bytes, 4);
    push_u32(&mut bytes, COMPATIBLE_OFFSET);

    let value_offset = bytes.len();

    bytes.extend_from_slice(&[0xaa, 0xbb]);

    assert_eq!(
      Structure::new(&bytes, &strings()).unwrap_err(),
      StructureError::Read(ReadError::Truncated {
        offset: value_offset,
        requested: 4,
        remaining: 2,
      })
    );
  }

  // Token/tree termination
  #[test]
  fn invalid_token_inside_tree_is_rejected() {
    let mut bytes = Vec::new();

    push_begin_node(&mut bytes, b"");

    let offset = bytes.len();
    push_u32(&mut bytes, 0xdead_beef);

    assert_eq!(
      Structure::new(&bytes, &strings()).unwrap_err(),
      StructureError::InvalidToken {
        offset,
        value: 0xdead_beef,
      }
    );
  }

  #[test]
  fn end_token_before_root_closes_is_rejected() {
    let mut bytes = Vec::new();

    push_begin_node(&mut bytes, b"");

    let offset = push_end(&mut bytes);

    assert_eq!(
      Structure::new(&bytes, &strings()).unwrap_err(),
      StructureError::PrematureEnd { offset, depth: 1 }
    );
  }

  #[test]
  fn end_token_inside_nested_node_reports_current_depth() {
    let mut bytes = Vec::new();

    push_begin_node(&mut bytes, b"");
    push_begin_node(&mut bytes, b"child");

    let offset = push_end(&mut bytes);

    assert_eq!(
      Structure::new(&bytes, &strings()).unwrap_err(),
      StructureError::PrematureEnd { offset, depth: 2 }
    );
  }

  #[test]
  fn root_must_be_followed_by_end_token() {
    let mut bytes = Vec::new();

    push_begin_node(&mut bytes, b"");
    push_end_node(&mut bytes);

    let offset = push_token(&mut bytes, Token::BeginNode);

    assert_eq!(
      Structure::new(&bytes, &strings()).unwrap_err(),
      StructureError::ExpectedEnd {
        offset,
        found: Token::BeginNode.value(),
      }
    );
  }

  #[test]
  fn data_after_end_token_is_rejected() {
    let mut bytes = minimal_structure();

    let trailing_offset = bytes.len();
    bytes.extend_from_slice(&[0xde, 0xad, 0xbe, 0xef]);

    assert_eq!(
      Structure::new(&bytes, &strings()).unwrap_err(),
      StructureError::TrailingData {
        offset: trailing_offset,
        remaining: 4,
      }
    );
  }

  #[test]
  fn missing_end_token_is_reported_as_truncation() {
    let mut bytes = Vec::new();

    push_begin_node(&mut bytes, b"");
    push_end_node(&mut bytes);

    let expected_offset = bytes.len();

    assert_eq!(
      Structure::new(&bytes, &strings()).unwrap_err(),
      StructureError::Read(ReadError::Truncated {
        offset: expected_offset,
        requested: 4,
        remaining: 0,
      })
    );
  }
}
