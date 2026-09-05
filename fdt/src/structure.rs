//! Parsing and structural validation of the FDT structure block.
//!
//! This module validates the serialized structure block and establishes the
//! invariants represented by [`Structure`]. These invariants are sufficient for
//! the generic navigation API to traverse nodes and properties safely and
//! deterministically.
//!
//! Devicetree-wide semantic relationships that are not required for generic
//! traversal are validated separately using the views built on top of
//! [`Structure`].
//!
//! The Devicetree Specification requires alignment padding bytes to be zero.
//! This parser does not validate their contents and accepts nonzero padding for
//! compatibility with DTBs encountered in practice.
//!
//! Implementation based on [Devicetree Specification v0.4](https://www.devicetree.org/).

mod name;
mod semantic;
mod view;

use core::fmt;

use crate::{
  helpers,
  reader::{ReadError, Reader},
  strings::{PropertyNameError, Strings},
};

use name::validate;

pub use name::NodeNameError;
pub use view::{Children, Descendants, Node, Properties, Property};

/// An error encountered while validating an FDT structure block.
#[derive(Debug, PartialEq, Eq)]
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
    /// Byte offset of the node's `FDT_BEGIN_NODE` token.
    token_offset: usize,

    /// Byte offset at which the node name begins.
    name_offset: usize,

    /// Tree depth of the parent containing the invalid node.
    ///
    /// The root node has depth `1`.
    parent_depth: usize,

    /// Reason the node name is invalid.
    source: NodeNameError,
  },

  /// A node has a unit-address but does not contain a `reg` property.
  UnitAddressWithoutReg,

  /// A node contains more than one child with the same full node name.
  DuplicateNodeName {
    /// Byte offset of the first node token within the structure block.
    first_node_offset: usize,

    /// Byte offset of the duplicate node token within the structure block.
    duplicate_node_offset: usize,

    /// Tree depth of the parent containing the duplicate nodes.
    ///
    /// The root node has depth `1`.
    parent_depth: usize,
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

  /// A node contains more than one property with the same name.
  DuplicatePropertyName {
    /// Byte offset of the first property token within the structure block.
    first_property_offset: usize,

    /// Byte offset of the duplicate property token within the structure block.
    duplicate_property_offset: usize,

    /// Raw strings-block offset encoded by the duplicate property.
    name_offset: u32,

    /// Tree depth at which the duplicate property was encountered.
    ///
    /// The root node has depth `1`.
    depth: usize,
  },

  /// The structure terminates before all open nodes have been closed.
  PrematureEnd {
    /// Byte offset of the premature `FDT_END` token.
    offset: usize,

    /// Number of nodes still open when `FDT_END` was encountered.
    ///
    /// The root node has depth `1`.
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

  /// The root node does not contain the required `cpus` child.
  MissingCpusNode,

  /// The root node contains no child whose node-name component is `memory`.
  MissingMemoryNode,
}

impl From<ReadError> for StructureError {
  fn from(error: ReadError) -> Self {
    Self::Read(error)
  }
}

/// A structurally validated view of an FDT structure block.
///
/// A `Structure` represents a structure block that has passed the validation
/// required by the generic navigation API. Instances are constructed through
/// [`Structure::new`].
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
  const fn parse(value: u32, offset: usize) -> Result<Self, StructureError> {
    if value == Self::BeginNode.value() {
      Ok(Self::BeginNode)
    } else if value == Self::EndNode.value() {
      Ok(Self::EndNode)
    } else if value == Self::Property.value() {
      Ok(Self::Property)
    } else if value == Self::Nop.value() {
      Ok(Self::Nop)
    } else if value == Self::End.value() {
      Ok(Self::End)
    } else {
      Err(StructureError::InvalidToken { offset, value })
    }
  }

  /// Returns the encoded 32-bit value of this token.
  #[expect(
    clippy::as_conversions,
    reason = "`Token` has repr(u32), so conversion yields its encoded discriminant"
  )]
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

/// Searches the already validated structure prefix for a prior sibling whose
/// full node name equals `name`.
///
/// `structure_start..node_offset` must contain the already validated portion
/// of the structure beginning immediately after the root node name.
///
/// `parent_depth` is the depth of the parent containing the current node.
///
/// Returns the structure-block offset of the matching sibling node token, or
/// `None` if no sibling with `name` has appeared.
///
/// # Errors
///
/// Returns [`StructureError::Read`] if the validated prefix cannot be read or
/// [`StructureError::InvalidToken`] if an invalid token is encountered.
fn find_prior_sibling_node(
  bytes: &[u8],
  structure_start: usize,
  node_offset: usize,
  parent_depth: usize,
  name: &[u8],
) -> Result<Option<usize>, StructureError> {
  let mut reader = Reader::new(bytes);

  reader.read_bytes(structure_start)?;

  // The root is already open at `structure_start`.
  let mut depth = 1usize;

  let mut matching_sibling = None;

  while reader.position() < node_offset {
    let (token_offset, token) = read_token(&mut reader)?;

    match token {
      Token::BeginNode => {
        let node_name = reader.read_nul_terminated()?;

        // Padding contents are ignored.
        reader.align_to_4()?;

        if depth == parent_depth && node_name == name {
          matching_sibling = Some(token_offset);
        }

        #[expect(
          clippy::arithmetic_side_effects,
          reason = "the prefix was already structurally validated"
        )]
        {
          depth += 1;
        }

        // Entering a node whose resulting depth equals `parent_depth`
        // means that node is a possible parent of the current node.
        //
        // Any match belonging to an earlier parent at the same depth
        // must therefore be discarded.
        if depth == parent_depth {
          matching_sibling = None;
        }
      }

      #[expect(
        clippy::arithmetic_side_effects,
        reason = "the prefix was already structurally validated"
      )]
      Token::EndNode => {
        depth -= 1;
      }

      Token::Property => {
        let length = reader.read_u32()?;

        // nameoff
        reader.read_u32()?;

        reader.read_bytes(helpers::usize_from_u32(length))?;

        // Padding contents are ignored.
        reader.align_to_4()?;
      }

      Token::Nop => {}

      #[expect(
        clippy::unreachable,
        reason = "FDT_END cannot occur in the already validated structure prefix"
      )]
      Token::End => {
        unreachable!("validated structure prefix cannot contain FDT_END");
      }
    }
  }

  Ok(matching_sibling)
}

/// Searches an already validated list of properties for `name`.
///
/// `properties_start..properties_end` must describe the portion of the
/// current node already consumed while it remained in the properties phase.
/// Every non-NOP token in the range is a property token.
///
/// Returns the structure-block offset of the matching property token, or
/// `None` if no property with `name` has appeared.
///
/// # Errors
///
/// Returns [`StructureError::Read`] if the property prefix cannot be read,
/// [`StructureError::InvalidToken`] if an invalid token is encountered, or
/// [`StructureError::InvalidPropertyName`] if a property references an invalid
/// name.
fn find_property(
  bytes: &[u8],
  properties_start: usize,
  properties_end: usize,
  name: &[u8],
  strings: &Strings<'_>,
) -> Result<Option<usize>, StructureError> {
  let mut reader = Reader::new(bytes);

  reader.read_bytes(properties_start)?;

  while reader.position() < properties_end {
    let (token_offset, token) = read_token(&mut reader)?;

    if token == Token::Nop {
      continue;
    }

    // `properties_start..properties_end` has already been consumed by
    // `Structure::new` while the node was in `NodePhase::Properties`.
    // Therefore, every non-NOP token in this range is FDT_PROP.
    let length = reader.read_u32()?;
    let name_offset = reader.read_u32()?;

    let Ok(property_name) = strings.validate_property_name(name_offset) else {
      #[expect(
        clippy::unreachable,
        reason = "property names in the searched prefix were already validated by Structure::new"
      )]
      {
        unreachable!("validated property prefix contains an invalid property-name reference");
      }
    };

    if property_name == name {
      return Ok(Some(token_offset));
    }

    reader.read_bytes(helpers::usize_from_u32(length))?;
    reader.align_to_4()?;
  }

  Ok(None)
}

/// Reads and validates one property from the current node.
///
/// The property token itself has already been consumed. `properties_start`
/// identifies the beginning of the current node's property prefix and
/// `token_offset` identifies the current property token.
///
/// # Errors
///
/// Returns [`StructureError::Read`] if the property header, value, or padding
/// is truncated, [`StructureError::InvalidPropertyName`] if the encoded
/// property name is invalid, or [`StructureError::DuplicatePropertyName`] if
/// the current node already contains a property with the same name.
fn validate_property(
  reader: &mut Reader<'_>,
  bytes: &[u8],
  strings: &Strings<'_>,
  properties_start: usize,
  token_offset: usize,
  depth: usize,
) -> Result<(), StructureError> {
  let length = reader.read_u32()?;
  let name_offset = reader.read_u32()?;

  let name = strings
    .validate_property_name(name_offset)
    .map_err(|source| StructureError::InvalidPropertyName {
      property_offset: token_offset,
      name_offset,
      source,
    })?;

  reader.read_bytes(helpers::usize_from_u32(length))?;

  // Padding contents are ignored.
  reader.align_to_4()?;

  if let Some(first_property_offset) =
    find_property(bytes, properties_start, token_offset, name, strings)?
  {
    return Err(StructureError::DuplicatePropertyName {
      first_property_offset,
      duplicate_property_offset: token_offset,
      name_offset,
      depth,
    });
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
  /// Validates an FDT structure block and constructs a [`Structure`].
  ///
  /// Validation establishes the representation-level invariants required by the
  /// generic navigation API, including valid token grammar and nesting, valid
  /// node and property names, bounded property data and alignment, property
  /// ordering, and uniqueness of properties and sibling node names.
  ///
  /// Devicetree-wide semantic relationships that are not required for traversal
  /// are to be validated separately.
  ///
  /// `bytes` is interpreted as one complete structure block, while `strings`
  /// supplies the strings block referenced by property-name offsets.
  ///
  /// # Errors
  ///
  /// Returns a [`StructureError`] if any of these requirements are violated.
  #[expect(
    clippy::too_many_lines,
    reason = "structure validation is a single state-machine pass whose readability would not improve by splitting it"
  )]
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
    let first_byte = reader.read_u8()?;

    if first_byte != 0 {
      return Err(StructureError::RootNameNotEmpty {
        offset: root_name_offset,
        first_byte,
      });
    }

    // Padding contents are ignored.
    reader.align_to_4()?;

    let root_content_start = reader.position();
    let mut properties_start = reader.position();

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

          validate(name).map_err(|source| StructureError::InvalidNodeName {
            token_offset,
            name_offset,
            parent_depth: depth,
            source,
          })?;

          // Padding contents are ignored.
          reader.align_to_4()?;

          if let Some(first_node_offset) =
            find_prior_sibling_node(bytes, root_content_start, token_offset, depth, name)?
          {
            return Err(StructureError::DuplicateNodeName {
              first_node_offset,
              duplicate_node_offset: token_offset,
              parent_depth: depth,
            });
          }

          #[expect(
            clippy::arithmetic_side_effects,
            reason = "each depth increase requires consuming bytes from a slice whose length is bounded by usize::MAX"
          )]
          {
            depth += 1;
          }

          phase = NodePhase::Properties;
          properties_start = reader.position();
        }

        #[expect(
          clippy::arithmetic_side_effects,
          reason = "the enclosing loop guarantees depth is greater than zero"
        )]
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

          validate_property(
            &mut reader,
            bytes,
            strings,
            properties_start,
            token_offset,
            depth,
          )?;
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
mod test_utils;

#[cfg(test)]
mod tests {
  use super::*;
  use crate::structure::test_utils::*;

  extern crate std;
  use std::vec::Vec;

  const COMPATIBLE_OFFSET: u32 = 0;

  fn minimal_structure() -> Vec<u8> {
    let mut bytes = Vec::new();

    push_begin_node(&mut bytes, b"");

    push_required_root_nodes(&mut bytes);

    push_end_node(&mut bytes);
    push_end(&mut bytes);

    bytes
  }

  #[test]
  fn debug_reports_structure_size() {
    let bytes = minimal_structure();
    let structure = Structure::new(&bytes, &strings()).expect("valid structure should parse");

    assert_eq!(std::format!("{structure:?}"), "Structure { size: 68 }");
  }

  // Node names
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

  // Valid structure blocks
  #[test]
  fn structure_with_properties_and_children_is_valid() {
    let mut bytes = Vec::new();

    push_begin_node(&mut bytes, b"");

    push_property(&mut bytes, COMPATIBLE_OFFSET, b"test,root");

    push_required_root_nodes(&mut bytes);

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

    push_required_root_nodes(&mut bytes);

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
    push_required_root_nodes(&mut bytes);
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
        parent_depth: 1,
        source: NodeNameError::InvalidFirstCharacter { byte: b'1' },
      }
    );
  }

  #[test]
  fn duplicate_sibling_node_name_is_rejected() {
    let mut bytes = Vec::new();

    push_begin_node(&mut bytes, b"");

    let first_node_offset = push_begin_node(&mut bytes, b"serial@1000");
    push_property(&mut bytes, REG_OFFSET, b"1000");
    push_end_node(&mut bytes);

    let duplicate_node_offset = push_begin_node(&mut bytes, b"serial@1000");
    push_property(&mut bytes, REG_OFFSET, b"1000");
    push_end_node(&mut bytes);

    push_end_node(&mut bytes);
    push_end(&mut bytes);

    assert_eq!(
      Structure::new(&bytes, &strings()).unwrap_err(),
      StructureError::DuplicateNodeName {
        first_node_offset,
        duplicate_node_offset,
        parent_depth: 1,
      }
    );
  }

  #[test]
  fn same_node_name_under_different_parents_is_valid() {
    let mut bytes = Vec::new();

    push_begin_node(&mut bytes, b"");

    push_required_root_nodes(&mut bytes);

    push_begin_node(&mut bytes, b"first");

    push_begin_node(&mut bytes, b"serial@1000");
    push_property(&mut bytes, REG_OFFSET, b"1000");
    push_end_node(&mut bytes);

    push_end_node(&mut bytes);

    push_begin_node(&mut bytes, b"second");

    push_begin_node(&mut bytes, b"serial@1000");
    push_property(&mut bytes, REG_OFFSET, b"1000");
    push_end_node(&mut bytes);

    push_end_node(&mut bytes);

    push_end_node(&mut bytes);
    push_end(&mut bytes);

    assert!(Structure::new(&bytes, &strings()).is_ok());
  }

  #[test]
  fn same_node_name_with_different_unit_addresses_is_valid() {
    let mut bytes = Vec::new();

    push_begin_node(&mut bytes, b"");

    push_required_root_nodes(&mut bytes);

    push_begin_node(&mut bytes, b"serial@1000");
    push_property(&mut bytes, REG_OFFSET, b"1000");
    push_end_node(&mut bytes);

    push_begin_node(&mut bytes, b"serial@2000");
    push_property(&mut bytes, REG_OFFSET, b"2000");
    push_end_node(&mut bytes);

    push_end_node(&mut bytes);
    push_end(&mut bytes);

    assert!(Structure::new(&bytes, &strings()).is_ok());
  }

  #[test]
  fn duplicate_node_name_in_nested_parent_is_rejected() {
    let mut bytes = Vec::new();

    push_begin_node(&mut bytes, b"");

    push_begin_node(&mut bytes, b"soc");

    let first_node_offset = push_begin_node(&mut bytes, b"serial@1000");
    push_property(&mut bytes, REG_OFFSET, b"1000");
    push_end_node(&mut bytes);

    let duplicate_node_offset = push_begin_node(&mut bytes, b"serial@1000");
    push_property(&mut bytes, REG_OFFSET, b"1000");
    push_end_node(&mut bytes);

    push_end_node(&mut bytes);

    push_end_node(&mut bytes);
    push_end(&mut bytes);

    assert_eq!(
      Structure::new(&bytes, &strings()).unwrap_err(),
      StructureError::DuplicateNodeName {
        first_node_offset,
        duplicate_node_offset,
        parent_depth: 2,
      }
    );
  }

  #[test]
  fn same_node_name_at_different_parent_depth_with_nops_is_valid() {
    let mut bytes = Vec::new();

    push_begin_node(&mut bytes, b"");

    push_required_root_nodes(&mut bytes);

    push_begin_node(&mut bytes, b"wrapper");

    push_nop(&mut bytes);
    push_nop(&mut bytes);
    push_nop(&mut bytes);

    push_begin_node(&mut bytes, b"serial@1000");
    push_property(&mut bytes, REG_OFFSET, b"1000");
    push_end_node(&mut bytes);

    push_nop(&mut bytes);
    push_nop(&mut bytes);
    push_nop(&mut bytes);

    push_end_node(&mut bytes);

    push_begin_node(&mut bytes, b"serial@1000");
    push_property(&mut bytes, REG_OFFSET, b"1000");
    push_end_node(&mut bytes);

    push_end_node(&mut bytes);
    push_end(&mut bytes);

    assert!(Structure::new(&bytes, &strings()).is_ok());
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
  fn duplicate_property_name_is_rejected() {
    let mut bytes = Vec::new();

    push_begin_node(&mut bytes, b"");

    let first_property_offset = push_property(&mut bytes, COMPATIBLE_OFFSET, b"first");

    let duplicate_property_offset = push_property(&mut bytes, COMPATIBLE_OFFSET, b"second");

    push_end_node(&mut bytes);
    push_end(&mut bytes);

    assert_eq!(
      Structure::new(&bytes, &strings()).unwrap_err(),
      StructureError::DuplicatePropertyName {
        first_property_offset,
        duplicate_property_offset,
        name_offset: COMPATIBLE_OFFSET,
        depth: 1,
      }
    );
  }

  #[test]
  fn same_property_name_in_different_nodes_is_valid() {
    let mut bytes = Vec::new();

    push_begin_node(&mut bytes, b"");

    push_required_root_nodes(&mut bytes);

    push_begin_node(&mut bytes, b"first");
    push_property(&mut bytes, COMPATIBLE_OFFSET, b"first");
    push_end_node(&mut bytes);

    push_begin_node(&mut bytes, b"second");
    push_property(&mut bytes, COMPATIBLE_OFFSET, b"second");
    push_end_node(&mut bytes);

    push_end_node(&mut bytes);
    push_end(&mut bytes);

    assert!(Structure::new(&bytes, &strings()).is_ok());
  }

  #[test]
  fn duplicate_property_names_at_different_string_offsets_are_rejected() {
    const DUPLICATE_OFFSET: u32 = 11;

    let strings = Strings::new(b"compatible\0compatible\0");

    let mut bytes = Vec::new();

    push_begin_node(&mut bytes, b"");

    let first_property_offset = push_property(&mut bytes, 0, b"first");

    let duplicate_property_offset = push_property(&mut bytes, DUPLICATE_OFFSET, b"second");

    push_end_node(&mut bytes);
    push_end(&mut bytes);

    assert_eq!(
      Structure::new(&bytes, &strings).unwrap_err(),
      StructureError::DuplicatePropertyName {
        first_property_offset,
        duplicate_property_offset,
        name_offset: DUPLICATE_OFFSET,
        depth: 1,
      }
    );
  }

  #[test]
  fn nop_in_property_prefix_is_ignored() {
    let mut bytes = Vec::new();

    push_begin_node(&mut bytes, b"");

    push_property(&mut bytes, REG_OFFSET, &[0xaa]);

    push_nop(&mut bytes);

    let first_property_offset = push_property(&mut bytes, COMPATIBLE_OFFSET, b"first");

    let duplicate_property_offset = push_property(&mut bytes, COMPATIBLE_OFFSET, b"second");

    push_end_node(&mut bytes);
    push_end(&mut bytes);

    assert_eq!(
      Structure::new(&bytes, &strings()).unwrap_err(),
      StructureError::DuplicatePropertyName {
        first_property_offset,
        duplicate_property_offset,
        name_offset: COMPATIBLE_OFFSET,
        depth: 1,
      }
    );
  }

  #[test]
  fn duplicate_property_is_found_after_different_property() {
    let mut bytes = Vec::new();

    push_begin_node(&mut bytes, b"");

    push_property(&mut bytes, REG_OFFSET, &[0xaa]);

    let first_property_offset = push_property(&mut bytes, COMPATIBLE_OFFSET, b"first");

    let duplicate_property_offset = push_property(&mut bytes, COMPATIBLE_OFFSET, b"second");

    push_end_node(&mut bytes);
    push_end(&mut bytes);

    assert_eq!(
      Structure::new(&bytes, &strings()).unwrap_err(),
      StructureError::DuplicatePropertyName {
        first_property_offset,
        duplicate_property_offset,
        name_offset: COMPATIBLE_OFFSET,
        depth: 1,
      }
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
    push_required_root_nodes(&mut bytes);
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
