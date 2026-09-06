//! Validation and interpretation of generic Devicetree addressing semantics.

use crate::{helpers, structure::Node};

use super::SemanticError;

/// Number of bytes in one Devicetree cell.
const CELL_SIZE: usize = size_of::<u32>();

/// Property that specifies the number of address cells used by direct children.
const ADDRESS_CELLS_PROPERTY: &[u8] = b"#address-cells";

/// Property that specifies the number of size cells used by direct children.
const SIZE_CELLS_PROPERTY: &[u8] = b"#size-cells";

/// Number of 32-bit cells used to encode an address for a node's direct child.
#[derive(Clone, Copy)]
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
#[derive(Clone, Copy)]
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

/// Byte layout of one `reg` entry for a node.
///
/// A `reg` entry consists of an address field followed by a size field. Their
/// widths are derived from the effective `#address-cells` and `#size-cells`
/// values established by the parent node.
#[derive(Clone, Copy)]
pub(super) struct RegLayout {
  /// Width of the address field in bytes.
  address_size: usize,

  /// Total width of one address-size pair in bytes.
  entry_size: usize,
}

impl RegLayout {
  /// Computes the byte layout of one `reg` entry from child-addressing
  /// information.
  ///
  /// # Errors
  ///
  /// Returns [`SemanticError::RegEntrySizeOverflow`] if either field width or
  /// the complete entry width cannot be represented as a [`usize`].
  pub(super) fn new(addressing: ChildAddressing) -> Result<Self, SemanticError> {
    let address_cells = addressing.address_cells().get();
    let size_cells = addressing.size_cells().get();

    let address_size = helpers::usize_from_u32(address_cells)
      .checked_mul(CELL_SIZE)
      .ok_or(SemanticError::RegEntrySizeOverflow {
        address_cells,
        size_cells,
      })?;

    let size_size = helpers::usize_from_u32(size_cells)
      .checked_mul(CELL_SIZE)
      .ok_or(SemanticError::RegEntrySizeOverflow {
        address_cells,
        size_cells,
      })?;

    let entry_size =
      address_size
        .checked_add(size_size)
        .ok_or(SemanticError::RegEntrySizeOverflow {
          address_cells,
          size_cells,
        })?;

    Ok(Self {
      address_size,
      entry_size,
    })
  }

  /// Returns the width of the address field in bytes.
  #[must_use]
  pub(super) const fn address_size(self) -> usize {
    self.address_size
  }

  /// Returns the total width of one `reg` entry in bytes.
  #[must_use]
  pub(super) const fn entry_size(self) -> usize {
    self.entry_size
  }
}

/// Address and size encoding established by a node for its direct children.
///
/// The values are taken from the node's `#address-cells` and `#size-cells`
/// properties. Missing properties use the effective defaults of two address
/// cells and one size cell. Values are not inherited from ancestor nodes.
#[derive(Clone, Copy)]
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
/// exactly one big-endian `u32`. Every `reg` property must contain a whole
/// number of entries encoded using the cell counts established by its parent.
///
/// # Errors
///
/// Returns a [`SemanticError`] when a generic addressing rule is violated.
pub(super) fn validate(root: &Node<'_>) -> Result<(), SemanticError> {
  validate_subtree(root)
}

/// Validates `parent` and recursively validates its descendants.
///
/// The cell counts established by `parent` are used to validate `reg`
/// properties belonging to its direct children.
///
/// # Errors
///
/// Returns a [`SemanticError`] when `parent` or one of its descendants
/// violates a generic addressing rule.
fn validate_subtree(parent: &Node<'_>) -> Result<(), SemanticError> {
  validate_node(parent)?;

  let addressing = child_addressing(parent);

  for child in parent.children() {
    validate_reg(&child, addressing)?;
    validate_subtree(&child)?;
  }

  Ok(())
}

/// Validates the encoding of a node's `reg` property.
///
/// Each `reg` entry consists of the number of address cells and size cells
/// established by the node's parent. A size-cell count of zero therefore
/// produces entries with no size field.
///
/// Nodes without a `reg` property require no validation.
///
/// # Errors
///
/// Returns [`SemanticError::RegEntrySizeOverflow`] if the parent cell counts
/// cannot be converted into a representable entry size.
///
/// Returns [`SemanticError::InvalidRegLength`] if the property value cannot be
/// divided into complete entries.
fn validate_reg(node: &Node<'_>, parent_addressing: ChildAddressing) -> Result<(), SemanticError> {
  let Some(reg) = node.property(b"reg") else {
    return Ok(());
  };

  let layout = RegLayout::new(parent_addressing)?;
  let entry_size = layout.entry_size();
  let length = reg.value().len();

  if entry_size == 0 {
    if length != 0 {
      return Err(SemanticError::InvalidRegLength { length, entry_size });
    }

    return Ok(());
  }

  if !length.is_multiple_of(entry_size) {
    return Err(SemanticError::InvalidRegLength { length, entry_size });
  }

  Ok(())
}

/// Validates generic addressing rules that depend only on `node`.
///
/// # Errors
///
/// Returns a [`SemanticError`] if the node contains malformed cell-count
/// properties or violates another node-local addressing rule.
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
    push_property(
      &mut bytes,
      REG_OFFSET,
      &[
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10, 0x00, 0x00, 0x00, 0x01, 0x00,
      ],
    );
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
  #[test]
  fn reg_absent_is_accepted() {
    let mut bytes = Vec::new();

    push_begin_node(&mut bytes, b"");

    push_begin_node(&mut bytes, b"device");
    push_end_node(&mut bytes);

    push_end_node(&mut bytes);
    push_end(&mut bytes);

    assert_eq!(validate_structure(&bytes), Ok(()));
  }

  #[test]
  fn single_reg_entry_is_accepted() {
    let mut bytes = Vec::new();

    push_begin_node(&mut bytes, b"");

    // Root defaults to two address cells and one size cell.
    push_begin_node(&mut bytes, b"device@1000");
    push_reg_cells(&mut bytes, &[0x0000_0000, 0x0000_1000, 0x0000_0100]);
    push_end_node(&mut bytes);

    push_end_node(&mut bytes);
    push_end(&mut bytes);

    assert_eq!(validate_structure(&bytes), Ok(()));
  }

  #[test]
  fn multiple_reg_entries_are_accepted() {
    let mut bytes = Vec::new();

    push_begin_node(&mut bytes, b"");

    // Each entry contains two address cells followed by one size cell.
    push_begin_node(&mut bytes, b"device@1000");
    push_reg_cells(
      &mut bytes,
      &[
        0x0000_0000,
        0x0000_1000,
        0x0000_0100,
        0x0000_0000,
        0x0000_2000,
        0x0000_0200,
      ],
    );
    push_end_node(&mut bytes);

    push_end_node(&mut bytes);
    push_end(&mut bytes);

    assert_eq!(validate_structure(&bytes), Ok(()));
  }

  #[test]
  fn incomplete_reg_entry_is_rejected() {
    let mut bytes = Vec::new();

    push_begin_node(&mut bytes, b"");

    push_begin_node(&mut bytes, b"device@1000");

    // Root defaults require three cells per entry, but only two are present.
    push_reg_cells(&mut bytes, &[0x0000_0000, 0x0000_1000]);

    push_end_node(&mut bytes);

    push_end_node(&mut bytes);
    push_end(&mut bytes);

    assert_eq!(
      validate_structure(&bytes),
      Err(SemanticError::InvalidRegLength {
        length: 8,
        entry_size: 12,
      })
    );
  }

  #[test]
  fn reg_uses_parent_cell_counts() {
    let mut bytes = Vec::new();

    push_begin_node(&mut bytes, b"");

    // Direct children use one address cell and one size cell.
    push_cell_counts(&mut bytes, 1, 1);

    push_begin_node(&mut bytes, b"device@1000");
    push_reg_cells(&mut bytes, &[0x0000_1000, 0x0000_0100]);
    push_end_node(&mut bytes);

    push_end_node(&mut bytes);
    push_end(&mut bytes);

    assert_eq!(validate_structure(&bytes), Ok(()));
  }

  #[test]
  fn reg_uses_immediate_parent_cell_counts() {
    let mut bytes = Vec::new();

    push_begin_node(&mut bytes, b"");

    // Root's direct children use one address and one size cell.
    push_cell_counts(&mut bytes, 1, 1);

    push_begin_node(&mut bytes, b"bus");

    // `bus` deliberately defines no cell counts. Its own children therefore
    // use the defaults of two address cells and one size cell.
    push_begin_node(&mut bytes, b"device@1000");
    push_reg_cells(&mut bytes, &[0x0000_0000, 0x0000_1000, 0x0000_0100]);
    push_end_node(&mut bytes);

    push_end_node(&mut bytes);

    push_end_node(&mut bytes);
    push_end(&mut bytes);

    assert_eq!(validate_structure(&bytes), Ok(()));
  }

  #[test]
  fn zero_size_cells_omits_size_field() {
    let mut bytes = Vec::new();

    push_begin_node(&mut bytes, b"");

    push_cell_counts(&mut bytes, 2, 0);

    push_begin_node(&mut bytes, b"device@1000");

    // With zero size cells, an entry contains only its two address cells.
    push_reg_cells(&mut bytes, &[0x0000_0000, 0x0000_1000]);

    push_end_node(&mut bytes);

    push_end_node(&mut bytes);
    push_end(&mut bytes);

    assert_eq!(validate_structure(&bytes), Ok(()));
  }

  #[test]
  fn zero_size_cells_rejects_size_field() {
    let mut bytes = Vec::new();

    push_begin_node(&mut bytes, b"");

    push_cell_counts(&mut bytes, 2, 0);

    push_begin_node(&mut bytes, b"device@1000");

    // Only two cells belong to an entry. Adding a third leaves an incomplete
    // second entry.
    push_reg_cells(&mut bytes, &[0x0000_0000, 0x0000_1000, 0x0000_0100]);

    push_end_node(&mut bytes);

    push_end_node(&mut bytes);
    push_end(&mut bytes);

    assert_eq!(
      validate_structure(&bytes),
      Err(SemanticError::InvalidRegLength {
        length: 12,
        entry_size: 8,
      })
    );
  }

  #[test]
  fn zero_width_reg_accepts_empty_value() {
    let mut bytes = Vec::new();

    push_begin_node(&mut bytes, b"");

    push_cell_counts(&mut bytes, 0, 0);

    push_begin_node(&mut bytes, b"device");
    push_property(&mut bytes, REG_OFFSET, &[]);
    push_end_node(&mut bytes);

    push_end_node(&mut bytes);
    push_end(&mut bytes);

    assert_eq!(validate_structure(&bytes), Ok(()));
  }

  #[test]
  fn zero_width_reg_rejects_nonempty_value() {
    let mut bytes = Vec::new();

    push_begin_node(&mut bytes, b"");

    push_cell_counts(&mut bytes, 0, 0);

    push_begin_node(&mut bytes, b"device");
    push_reg_cells(&mut bytes, &[0x0000_0000]);
    push_end_node(&mut bytes);

    push_end_node(&mut bytes);
    push_end(&mut bytes);

    assert_eq!(
      validate_structure(&bytes),
      Err(SemanticError::InvalidRegLength {
        length: 4,
        entry_size: 0,
      })
    );
  }
}
