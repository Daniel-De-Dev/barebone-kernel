//! Validation and interpretation of generic Devicetree addressing semantics.

use crate::structure::Node;

use super::SemanticError;

/// Property that specifies the number of address cells used by direct children.
const ADDRESS_CELLS_PROPERTY: &[u8] = b"#address-cells";

/// Property that specifies the number of size cells used by direct children.
const SIZE_CELLS_PROPERTY: &[u8] = b"#size-cells";

/// Number of 32-bit cells used to encode an address for a node's direct child.
pub(super) struct AddressCells(
  /// Number of cells in the encoded address field.
  u32,
);

impl AddressCells {
  /// Effective value used when `#address-cells` is absent.
  const DEFAULT: Self = Self(2);

  /// Returns the encoded field width as a number of 32-bit cells.
  #[must_use]
  pub(super) const fn get(self) -> u32 {
    self.0
  }
}

/// Number of 32-bit cells used to encode a size for a node's direct child.
pub(super) struct SizeCells(
  /// Number of cells in the encoded size field.
  u32,
);

impl SizeCells {
  /// Effective value used when `#size-cells` is absent.
  const DEFAULT: Self = Self(1);

  /// Returns the encoded field width as a number of 32-bit cells.
  #[must_use]
  pub(super) const fn get(self) -> u32 {
    self.0
  }
}

/// Address and size encoding established by a node for its direct children.
///
/// The values are taken from the node's `#address-cells` and `#size-cells`
/// properties. Missing properties use the effective defaults of two address
/// cells and one size cell. Values are not inherited from ancestor nodes.
pub(super) struct ChildAddressing {
  /// Number of cells used to encode addresses of direct children.
  address_cells: AddressCells,

  /// Number of cells used to encode sizes of direct children.
  size_cells: SizeCells,
}

impl ChildAddressing {
  /// Returns the number of cells used to encode addresses of direct children.
  #[must_use]
  pub(super) const fn address_cells(self) -> AddressCells {
    self.address_cells
  }

  /// Returns the number of cells used to encode sizes of direct children.
  #[must_use]
  pub(super) const fn size_cells(self) -> SizeCells {
    self.size_cells
  }
}

/// Validates generic addressing semantics across the entire tree.
///
/// Every explicit `#address-cells` and `#size-cells` property must contain
/// exactly one big-endian `u32`. Missing properties are valid and imply the
/// effective defaults of two address cells and one size cell.
///
/// Validation also checks the generic relationship between node unit addresses
/// and their addressing properties.
///
/// # Errors
///
/// Returns [`SemanticError::InvalidAddressCells`] or
/// [`SemanticError::InvalidSizeCells`] when an explicit cell-count property is
/// malformed, or another [`SemanticError`] when a generic addressing rule is
/// violated.
pub(super) fn validate(root: &Node<'_>) -> Result<(), SemanticError> {
  validate_node(root)?;

  for node in root.descendants() {
    validate_node(&node)?;
  }

  Ok(())
}

/// Validates all generic addressing rules enforced for one node.
///
/// This validates the node's child cell-count encoding and its own
/// unit-address relationship.
///
/// # Errors
///
/// Returns a [`SemanticError`] if the node contains malformed cell-count
/// properties or violates another generic addressing rule.
fn validate_node(node: &Node<'_>) -> Result<(), SemanticError> {
  parse_child_addressing(node)?;
  validate_node_addressing(node)
}

/// Returns the effective cell counts established by `node` for its children.
///
/// Explicit cell-count properties are decoded as big-endian `u32` values.
/// Missing properties use two address cells and one size cell.
///
/// # Panics
///
/// Panics if an explicit `#address-cells` or `#size-cells` property is
/// malformed. Callers must only use this function after generic addressing
/// semantics for the node have been validated.
pub(super) fn child_addressing(node: &Node<'_>) -> ChildAddressing {
  #[expect(
    clippy::expect_used,
    reason = "Fdt semantic validation guarantees cell-count properties contain exactly one u32"
  )]
  parse_child_addressing(node)
    .expect("validated node must contain valid address/size cell-count properties")
}

/// Parses the effective child-addressing configuration of `node`.
///
/// Explicit cell-count properties are decoded from their big-endian `u32`
/// representation. Missing properties use two address cells and one size cell.
///
/// # Errors
///
/// Returns [`SemanticError::InvalidAddressCells`] or
/// [`SemanticError::InvalidSizeCells`] if an explicit property does not contain
/// exactly one `u32`.
fn parse_child_addressing(node: &Node<'_>) -> Result<ChildAddressing, SemanticError> {
  Ok(ChildAddressing {
    address_cells: parse_address_cells(node)?,
    size_cells: parse_size_cells(node)?,
  })
}

/// Parses the effective `#address-cells` value of `node`.
///
/// Returns two when the property is absent.
///
/// # Errors
///
/// Returns [`SemanticError::InvalidAddressCells`] if the property exists but
/// does not contain exactly one big-endian `u32`.
fn parse_address_cells(node: &Node<'_>) -> Result<AddressCells, SemanticError> {
  let Some(property) = node.property(ADDRESS_CELLS_PROPERTY) else {
    return Ok(AddressCells::DEFAULT);
  };

  let value = decode_u32(property.value()).ok_or_else(|| SemanticError::InvalidAddressCells {
    length: property.value().len(),
  })?;

  Ok(AddressCells(value))
}

/// Parses the effective `#size-cells` value of `node`.
///
/// Returns one when the property is absent.
///
/// # Errors
///
/// Returns [`SemanticError::InvalidSizeCells`] if the property exists but
/// does not contain exactly one big-endian `u32`.
fn parse_size_cells(node: &Node<'_>) -> Result<SizeCells, SemanticError> {
  let Some(property) = node.property(SIZE_CELLS_PROPERTY) else {
    return Ok(SizeCells::DEFAULT);
  };

  let value = decode_u32(property.value()).ok_or_else(|| SemanticError::InvalidSizeCells {
    length: property.value().len(),
  })?;

  Ok(SizeCells(value))
}

/// Decodes one big-endian `u32` from `bytes`.
///
/// Returns `None` unless `bytes` contains exactly four bytes.
fn decode_u32(bytes: &[u8]) -> Option<u32> {
  let bytes: [u8; 4] = bytes.try_into().ok()?;

  Some(u32::from_be_bytes(bytes))
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
/// Returns [`SemanticError::UnitAddressWithoutReg`] if a node has a unit
/// address but no `reg` property.
// TODO: Add check that first entry of reg matches?
fn validate_node_addressing(node: &Node<'_>) -> Result<(), SemanticError> {
  if node.unit_address().is_none() {
    return Ok(());
  }

  let has_reg = node.property(b"reg").is_some();
  let has_ranges = node.property(b"ranges").is_some();

  if !has_reg && !has_ranges {
    return Err(SemanticError::UnitAddressWithoutReg);
  }

  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::structure::{Structure, test_utils::*};

  extern crate std;
  use std::vec::Vec;

  fn validate_structure(bytes: &[u8]) -> Result<(), SemanticError> {
    let strings = strings();

    let structure =
      Structure::new(bytes, &strings).expect("test structure should be structurally valid");

    let root = structure.root(strings);

    validate(&root)
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

    assert_eq!(validate_structure(&bytes), Ok(()));
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

    assert_eq!(validate_structure(&bytes), Ok(()));
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

    assert_eq!(
      validate_structure(&bytes),
      Err(SemanticError::UnitAddressWithoutReg)
    );
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

    assert_eq!(
      validate_structure(&bytes),
      Err(SemanticError::UnitAddressWithoutReg)
    );
  }

  #[test]
  fn explicit_cell_counts_are_decoded() {
    let mut bytes = Vec::new();

    push_begin_node(&mut bytes, b"");
    push_cell_counts(&mut bytes, 1, 2);
    push_required_root_nodes(&mut bytes);
    push_end_node(&mut bytes);
    push_end(&mut bytes);

    let strings = strings();
    let structure =
      Structure::new(&bytes, &strings).expect("test structure should be structurally valid");
    let root = structure.root(strings);

    assert_eq!(validate(&root), Ok(()));

    assert_eq!(child_addressing(&root).address_cells().get(), 1);
    assert_eq!(child_addressing(&root).size_cells().get(), 2);
  }

  #[test]
  fn missing_cell_counts_use_defaults() {
    let mut bytes = Vec::new();

    push_begin_node(&mut bytes, b"");
    push_required_root_nodes(&mut bytes);
    push_end_node(&mut bytes);
    push_end(&mut bytes);

    let strings = strings();
    let structure =
      Structure::new(&bytes, &strings).expect("test structure should be structurally valid");
    let root = structure.root(strings);

    assert_eq!(validate(&root), Ok(()));

    assert_eq!(child_addressing(&root).address_cells().get(), 2);
    assert_eq!(child_addressing(&root).size_cells().get(), 1);
  }

  #[test]
  fn invalid_address_cells_lengths_are_rejected() {
    let values: &[&[u8]] = &[&[], &[0, 0, 1], &[0, 0, 0, 1, 0, 0, 0, 2]];

    for value in values {
      let mut bytes = Vec::new();

      push_begin_node(&mut bytes, b"");
      push_property(&mut bytes, ADDRESS_CELLS_OFFSET, value);
      push_required_root_nodes(&mut bytes);
      push_end_node(&mut bytes);
      push_end(&mut bytes);

      assert_eq!(
        validate_structure(&bytes),
        Err(SemanticError::InvalidAddressCells {
          length: value.len(),
        })
      );
    }
  }

  #[test]
  fn invalid_size_cells_lengths_are_rejected() {
    let values: &[&[u8]] = &[&[], &[0, 0, 1], &[0, 0, 0, 1, 0, 0, 0, 2]];

    for value in values {
      let mut bytes = Vec::new();

      push_begin_node(&mut bytes, b"");
      push_property(&mut bytes, SIZE_CELLS_OFFSET, value);
      push_required_root_nodes(&mut bytes);
      push_end_node(&mut bytes);
      push_end(&mut bytes);

      assert_eq!(
        validate_structure(&bytes),
        Err(SemanticError::InvalidSizeCells {
          length: value.len(),
        })
      );
    }
  }

  #[test]
  fn cell_counts_are_not_inherited() {
    let mut bytes = Vec::new();

    push_begin_node(&mut bytes, b"");

    // Root establishes (1, 2) for its direct children.
    push_cell_counts(&mut bytes, 1, 2);

    push_begin_node(&mut bytes, b"bus");

    // `bus` deliberately has no cell-count properties.
    push_begin_node(&mut bytes, b"device");
    push_end_node(&mut bytes);

    push_end_node(&mut bytes);

    push_required_root_nodes(&mut bytes);

    push_end_node(&mut bytes);
    push_end(&mut bytes);

    let strings = strings();
    let structure =
      Structure::new(&bytes, &strings).expect("test structure should be structurally valid");
    let root = structure.root(strings);

    assert_eq!(validate(&root), Ok(()));

    assert_eq!(child_addressing(&root).address_cells().get(), 1);
    assert_eq!(child_addressing(&root).size_cells().get(), 2);

    let bus = root.child(b"bus").expect("test structure contains bus");

    // `bus` does not inherit (1, 2); its missing properties use the
    // client defaults instead.
    assert_eq!(child_addressing(&bus).address_cells().get(), 2);
    assert_eq!(child_addressing(&bus).size_cells().get(), 1);
  }
}
