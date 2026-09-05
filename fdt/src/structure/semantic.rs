//! Devicetree-wide semantic validation performed after structural validation.
//!
//! These checks are expressed through the generic navigation API rather than
//! by re-parsing the serialized structure block.

use super::{Structure, StructureError, view::Node};
use crate::strings::Strings;

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
  // TODO: Make a specific StructureError::SemanticError sub group/type?
  pub(crate) fn validate_semantics(&self, strings: Strings<'a>) -> Result<(), StructureError> {
    let root = self.root(strings);

    validate_required_root_nodes(&root)?;

    for node in root.descendants() {
      validate_node_addressing(&node)?;
    }

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
/// Returns [`StructureError::MissingCpusNode`] if `cpus` is absent, or
/// [`StructureError::MissingMemoryNode`] if no memory node is present.
fn validate_required_root_nodes(root: &Node<'_>) -> Result<(), StructureError> {
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
    return Err(StructureError::MissingCpusNode);
  }

  if !memory_seen {
    return Err(StructureError::MissingMemoryNode);
  }

  Ok(())
}

/// Validates the basic relationship between a node's unit address and `reg`.
///
/// A node whose name contains an `@unit-address` must contain a `reg` property.
///
/// This check does not yet verify that the unit address matches the first
/// address encoded by `reg`.
///
/// For compatibility with established devicetree tooling and generated trees,
/// a `ranges` property is also accepted.
///
/// # Errors
///
/// Returns [`StructureError::UnitAddressWithoutReg`] if a node has a unit
/// address but no `reg` property.
// TODO: Add check that first entry of reg matches?
fn validate_node_addressing(node: &Node<'_>) -> Result<(), StructureError> {
  if node.unit_address().is_none() {
    return Ok(());
  }

  let has_reg = node.property(b"reg").is_some();
  let has_ranges = node.property(b"ranges").is_some();

  if !has_reg && !has_ranges {
    return Err(StructureError::UnitAddressWithoutReg);
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

    assert_eq!(validate(&bytes), Err(StructureError::MissingCpusNode));
  }

  #[test]
  fn missing_memory_node_is_rejected() {
    let mut bytes = Vec::new();

    push_begin_node(&mut bytes, b"");

    push_begin_node(&mut bytes, b"cpus");
    push_end_node(&mut bytes);

    push_end_node(&mut bytes);
    push_end(&mut bytes);

    assert_eq!(validate(&bytes), Err(StructureError::MissingMemoryNode));
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

    assert_eq!(validate(&bytes), Err(StructureError::MissingCpusNode));
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

    assert_eq!(validate(&bytes), Err(StructureError::MissingMemoryNode));
  }

  #[test]
  fn unit_address_with_reg_is_accepted() {
    let mut bytes = Vec::new();

    push_begin_node(&mut bytes, b"");
    push_required_root_nodes(&mut bytes);

    push_begin_node(&mut bytes, b"device@1000");
    push_property(&mut bytes, REG_OFFSET, &[0, 0, 0x10, 0]);
    push_end_node(&mut bytes);

    push_end_node(&mut bytes);
    push_end(&mut bytes);

    assert_eq!(validate(&bytes), Ok(()));
  }

  #[test]
  fn unit_address_with_ranges_is_accepted() {
    let mut bytes = Vec::new();

    push_begin_node(&mut bytes, b"");
    push_required_root_nodes(&mut bytes);

    push_begin_node(&mut bytes, b"platform-bus@4000000");
    push_property(&mut bytes, RANGES_OFFSET, &[0, 0, 0, 1]);
    push_end_node(&mut bytes);

    push_end_node(&mut bytes);
    push_end(&mut bytes);

    assert_eq!(validate(&bytes), Ok(()));
  }

  #[test]
  fn unit_address_without_address_property_is_rejected() {
    let mut bytes = Vec::new();

    push_begin_node(&mut bytes, b"");
    push_required_root_nodes(&mut bytes);

    push_begin_node(&mut bytes, b"device@1000");
    push_end_node(&mut bytes);

    push_end_node(&mut bytes);
    push_end(&mut bytes);

    assert_eq!(validate(&bytes), Err(StructureError::UnitAddressWithoutReg));
  }

  #[test]
  fn nested_unit_address_without_address_property_is_rejected() {
    let mut bytes = Vec::new();

    push_begin_node(&mut bytes, b"");
    push_required_root_nodes(&mut bytes);

    push_begin_node(&mut bytes, b"soc");

    push_begin_node(&mut bytes, b"device@1000");
    push_end_node(&mut bytes);

    push_end_node(&mut bytes);

    push_end_node(&mut bytes);
    push_end(&mut bytes);

    assert_eq!(validate(&bytes), Err(StructureError::UnitAddressWithoutReg));
  }
}
