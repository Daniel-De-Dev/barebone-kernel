//! Devicetree-wide semantic validation performed after structural validation.
//!
//! Semantic validation operates on the generic node/property views established
//! by structural validation. It checks relationships and property encodings
//! whose validity depends on their meaning rather than on the serialized
//! structure-block format alone.

mod addressing;

use super::{Structure, StructureError, view::Node};
use crate::strings::Strings;

/// An error encountered while validating Devicetree-wide semantic rules.
#[derive(Debug, PartialEq, Eq)]
pub enum SemanticError {
  /// The root node does not contain the required `cpus` child.
  MissingCpusNode,

  /// The root node contains no child whose node-name component is `memory`.
  MissingMemoryNode,

  /// A node has a unit-address but does not contain the required addressing
  /// property.
  UnitAddressWithoutReg,

  /// `#address-cells` does not contain exactly one `<u32>`.
  InvalidAddressCells {
    /// Length of the property value in bytes.
    length: usize,
  },

  /// `#size-cells` does not contain exactly one `<u32>`.
  InvalidSizeCells {
    /// Length of the property value in bytes.
    length: usize,
  },

  /// The byte length of a `reg` property cannot be divided into complete
  /// address/size entries using the cell counts established by its parent.
  InvalidRegLength {
    /// Length of the `reg` property value in bytes.
    length: usize,

    /// Required length of one complete `reg` entry in bytes.
    entry_size: usize,
  },

  /// The cell counts established by a parent produce a `reg` entry size that
  /// cannot be represented by this implementation.
  RegEntrySizeOverflow {
    /// Number of cells used for the address field.
    address_cells: u32,

    /// Number of cells used for the size field.
    size_cells: u32,
  },
}

impl From<SemanticError> for StructureError {
  fn from(error: SemanticError) -> Self {
    Self::Semantic(error)
  }
}

impl<'a> Structure<'a> {
  /// Validates devicetree-wide semantic invariants that are not required for
  /// generic structure traversal.
  ///
  /// Operates on the structural guarantees established by [`Structure::new`].
  ///
  /// # Errors
  ///
  /// Returns a [`StructureError`] when one of the semantic invariants enforced
  /// by this parser is violated.
  pub(crate) fn validate_semantics(&self, strings: Strings<'a>) -> Result<(), StructureError> {
    let root = self.root(strings);

    validate_required_root_nodes(&root)?;
    addressing::validate(&root)?;

    Ok(())
  }
}

/// Validates required direct children of the root node.
///
/// The root must contain a child named `cpus` and at least one child whose
/// node-name component is `memory`.
///
/// Uniqueness of the `cpus` node follows from the structural invariant that
/// direct siblings cannot have duplicate full node names.
///
/// # Errors
///
/// Returns [`SemanticError::MissingCpusNode`] if `cpus` is absent, or
/// [`SemanticError::MissingMemoryNode`] if no memory node is present.
fn validate_required_root_nodes(root: &Node<'_>) -> Result<(), SemanticError> {
  let mut cpus_seen = false;
  let mut memory_seen = false;

  for child in root.children() {
    if child.name() == b"cpus" {
      cpus_seen = true;
    }

    if child.name_component() == b"memory" {
      memory_seen = true;
    }
  }

  if !cpus_seen {
    return Err(SemanticError::MissingCpusNode);
  }

  if !memory_seen {
    return Err(SemanticError::MissingMemoryNode);
  }

  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::structure::test_utils::*;

  extern crate std;
  use std::vec::Vec;

  fn validate(bytes: &[u8]) -> Result<(), StructureError> {
    let strings = strings();

    let structure =
      Structure::new(bytes, &strings).expect("test structure should be structurally valid");

    structure.validate_semantics(strings)
  }

  #[test]
  fn required_root_nodes_are_accepted() {
    let mut bytes = Vec::new();

    push_begin_node(&mut bytes, b"");
    push_required_root_nodes(&mut bytes);
    push_end_node(&mut bytes);
    push_end(&mut bytes);

    assert_eq!(validate(&bytes), Ok(()));
  }

  #[test]
  fn missing_cpus_node_is_rejected() {
    let mut bytes = Vec::new();

    push_begin_node(&mut bytes, b"");

    push_begin_node(&mut bytes, b"memory@0");
    push_property(&mut bytes, REG_OFFSET, b"0");
    push_end_node(&mut bytes);

    push_end_node(&mut bytes);
    push_end(&mut bytes);

    assert_eq!(
      validate(&bytes),
      Err(StructureError::Semantic(SemanticError::MissingCpusNode))
    );
  }

  #[test]
  fn missing_memory_node_is_rejected() {
    let mut bytes = Vec::new();

    push_begin_node(&mut bytes, b"");

    push_begin_node(&mut bytes, b"cpus");
    push_end_node(&mut bytes);

    push_end_node(&mut bytes);
    push_end(&mut bytes);

    assert_eq!(
      validate(&bytes),
      Err(StructureError::Semantic(SemanticError::MissingMemoryNode))
    );
  }

  #[test]
  fn nested_cpus_node_does_not_satisfy_requirement() {
    let mut bytes = Vec::new();

    push_begin_node(&mut bytes, b"");

    push_begin_node(&mut bytes, b"memory@0");
    push_property(&mut bytes, REG_OFFSET, b"0");

    push_begin_node(&mut bytes, b"cpus");
    push_end_node(&mut bytes);

    push_end_node(&mut bytes);

    push_end_node(&mut bytes);
    push_end(&mut bytes);

    assert_eq!(
      validate(&bytes),
      Err(StructureError::Semantic(SemanticError::MissingCpusNode))
    );
  }

  #[test]
  fn nested_memory_node_does_not_satisfy_requirement() {
    let mut bytes = Vec::new();

    push_begin_node(&mut bytes, b"");

    push_begin_node(&mut bytes, b"cpus");

    push_begin_node(&mut bytes, b"memory@0");
    push_property(&mut bytes, REG_OFFSET, b"0");
    push_end_node(&mut bytes);

    push_end_node(&mut bytes);

    push_end_node(&mut bytes);
    push_end(&mut bytes);

    assert_eq!(
      validate(&bytes),
      Err(StructureError::Semantic(SemanticError::MissingMemoryNode))
    );
  }
}
