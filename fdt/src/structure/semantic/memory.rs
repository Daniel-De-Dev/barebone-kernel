//! Validation and interpretation of `/memory` nodes.
//!
//! Memory nodes describe physical memory through address-size pairs stored in
//! their `reg` properties. This module validates the memory-specific invariants
//! required by the public memory-range view and provides an iterator that
//! flattens ranges across all root `/memory` nodes.

use crate::structure::{Children, Node};

use super::{
  SemanticError,
  addressing::{self, RegLayout},
};

/// Width of one Devicetree cell in bytes.
const CELL_SIZE: usize = size_of::<u32>();

/// Numeric radix of one 32-bit Devicetree cell.
const CELL_RADIX: u64 = 0x1_0000_0000;

/// Node-name component identifying physical-memory nodes.
const MEMORY_NODE_NAME: &[u8] = b"memory";

/// Name of the required memory-node `device_type` property.
const DEVICE_TYPE_PROPERTY: &[u8] = b"device_type";

/// Name of the property containing memory address-size pairs.
const REG_PROPERTY: &[u8] = b"reg";

/// A physical memory range described by a Devicetree `/memory` node.
///
/// The range describes memory reported by the Devicetree itself. Reservations
/// are not removed from the range.
#[derive(Debug, Clone, Copy)]
pub struct MemoryRange {
  /// Physical start address of the range.
  address: u64,

  /// Size of the range in bytes.
  size: u64,
}

impl MemoryRange {
  /// Returns the physical start address of the memory range.
  #[must_use]
  pub const fn address(self) -> u64 {
    self.address
  }

  /// Returns the size of the memory range in bytes.
  #[must_use]
  pub const fn size(self) -> u64 {
    self.size
  }
}

/// Iterator over physical memory ranges described by root `/memory` nodes.
///
/// Multiple address-size pairs within one `reg` property and ranges spread
/// across multiple `/memory` nodes are exposed as one flat sequence.
///
/// Memory reservations are not excluded from the yielded ranges.
pub struct MemoryRanges<'a> {
  /// Remaining direct children of the root node to inspect.
  nodes: Children<'a>,

  /// Range iterator for the memory node currently being consumed.
  current: Option<MemoryNodeRanges<'a>>,

  /// Layout used to decode root-child `reg` entries.
  layout: RegLayout,
}

/// Iterator over address-size pairs in one validated memory node's `reg`
/// property.
struct MemoryNodeRanges<'a> {
  /// Portion of the `reg` property that has not yet been consumed.
  bytes: &'a [u8],

  /// Byte layout of each `reg` entry.
  layout: RegLayout,
}

/// Validates the memory-specific semantics of root `/memory` nodes.
///
/// Every root child whose node-name component is `memory` must contain a
/// `device_type` property identifying it as memory and a `reg` property
/// describing at least one non-empty physical memory range.
///
/// The root must establish non-zero address and size cell counts for its
/// children. Generic `reg` layout validation is expected to have completed
/// before this function is called.
///
/// # Errors
///
/// Returns [`SemanticError::InvalidMemoryAddressCells`] if the effective root
/// `#address-cells` value is zero.
///
/// Returns [`SemanticError::InvalidMemorySizeCells`] if the effective root
/// `#size-cells` value is zero.
///
/// Returns [`SemanticError::RegEntrySizeOverflow`] if the root addressing
/// information cannot produce a representable `reg` layout.
///
/// Returns [`SemanticError::MissingMemoryDeviceType`] or
/// [`SemanticError::InvalidMemoryDeviceType`] for an invalid memory-node
/// `device_type`.
///
/// Returns [`SemanticError::MissingMemoryReg`] if a memory node lacks `reg`.
///
/// Returns [`SemanticError::EmptyMemoryReg`] if a memory node's `reg` property
/// contains no address-size pairs.
///
/// Returns [`SemanticError::MemoryAddressDoesNotFitU64`] or
/// [`SemanticError::MemorySizeDoesNotFitU64`] if a memory range cannot be
/// represented by this implementation.
///
/// Returns [`SemanticError::ZeroMemorySize`] if a `reg` entry describes a
/// zero-sized memory range.
pub(super) fn validate(root: &Node<'_>) -> Result<(), SemanticError> {
  let addressing = addressing::child_addressing(root);

  if addressing.address_cells().get() == 0 {
    return Err(SemanticError::InvalidMemoryAddressCells);
  }

  if addressing.size_cells().get() == 0 {
    return Err(SemanticError::InvalidMemorySizeCells);
  }

  let layout = RegLayout::new(addressing)?;

  for node in root
    .children()
    .filter(|node| node.name_component() == MEMORY_NODE_NAME)
  {
    validate_memory_node(&node, layout)?;
  }

  Ok(())
}

/// Validates the required properties of one `/memory` node.
///
/// # Errors
///
/// Returns [`SemanticError::MissingMemoryDeviceType`] if `device_type` is
/// absent, or [`SemanticError::InvalidMemoryDeviceType`] if it does not encode
/// `"memory"`.
///
/// Returns [`SemanticError::MissingMemoryReg`] if `reg` is absent.
///
/// Propagates errors encountered while validating the values encoded by `reg`.
fn validate_memory_node(node: &Node<'_>, layout: RegLayout) -> Result<(), SemanticError> {
  let Some(device_type) = node.property(DEVICE_TYPE_PROPERTY) else {
    return Err(SemanticError::MissingMemoryDeviceType);
  };

  if device_type.value() != b"memory\0" {
    return Err(SemanticError::InvalidMemoryDeviceType);
  }

  let Some(reg) = node.property(REG_PROPERTY) else {
    return Err(SemanticError::MissingMemoryReg);
  };

  validate_memory_reg(reg.value(), layout)
}

/// Validates that a memory-node `reg` property describes one or more non-empty
/// ranges representable by [`MemoryRange`].
///
/// The property is assumed to have already passed generic `reg` layout
/// validation.
///
/// # Errors
///
/// Returns [`SemanticError::EmptyMemoryReg`] if `reg` contains no entries.
///
/// Returns [`SemanticError::MemoryAddressDoesNotFitU64`] if an encoded address
/// cannot be represented as a `u64`.
///
/// Returns [`SemanticError::MemorySizeDoesNotFitU64`] if an encoded size cannot
/// be represented as a `u64`.
///
/// Returns [`SemanticError::ZeroMemorySize`] if an entry encodes a size of
/// zero.
fn validate_memory_reg(bytes: &[u8], layout: RegLayout) -> Result<(), SemanticError> {
  if bytes.is_empty() {
    return Err(SemanticError::EmptyMemoryReg);
  }

  for entry in bytes.chunks_exact(layout.entry_size()) {
    let (address, size) = entry.split_at(layout.address_size());

    if decode_cells_u64(address).is_none() {
      return Err(SemanticError::MemoryAddressDoesNotFitU64);
    }

    let Some(size) = decode_cells_u64(size) else {
      return Err(SemanticError::MemorySizeDoesNotFitU64);
    };

    if size == 0 {
      return Err(SemanticError::ZeroMemorySize);
    }
  }

  Ok(())
}

/// Decodes a sequence of big-endian 32-bit Devicetree cells into a `u64`.
///
/// Cells are interpreted as digits in radix 2^32, with the first cell being
/// the most significant.
///
/// Returns `None` if `bytes` does not contain a whole number of cells or if the
/// represented value exceeds [`u64::MAX`].
fn decode_cells_u64(bytes: &[u8]) -> Option<u64> {
  let (cells, remainder) = bytes.as_chunks::<CELL_SIZE>();

  if !remainder.is_empty() {
    return None;
  }

  let mut value = 0_u64;

  for cell in cells {
    let cell = u64::from(u32::from_be_bytes(*cell));

    value = value.checked_mul(CELL_RADIX)?;
    value = value.checked_add(cell)?;
  }

  Some(value)
}

/// Decodes cells that have already been validated as representable by a
/// `u64`.
///
/// # Panics
///
/// Panics if `bytes` do not encode a valid cell sequence representable by a
/// `u64`. Memory semantic validation guarantees this invariant before this
/// function is used.
#[expect(
  clippy::expect_used,
  reason = "memory semantic validation guarantees memory values fit in u64"
)]
fn decode_validated_cells_u64(bytes: &[u8]) -> u64 {
  decode_cells_u64(bytes).expect("validated memory value must fit in u64")
}

impl<'a> MemoryNodeRanges<'a> {
  /// Creates an iterator over the `reg` entries of a validated memory node.
  ///
  /// # Panics
  ///
  /// Panics if `node` does not contain a `reg` property. Memory semantic
  /// validation guarantees that every memory node contains `reg` before this
  /// constructor is used.
  #[expect(
    clippy::expect_used,
    reason = "memory semantic validation guarantees every memory node contains reg"
  )]
  fn new(node: &Node<'a>, layout: RegLayout) -> Self {
    let bytes = node
      .property(REG_PROPERTY)
      .expect("validated memory node must contain reg")
      .value();

    Self { bytes, layout }
  }
}

impl Iterator for MemoryNodeRanges<'_> {
  type Item = MemoryRange;

  fn next(&mut self) -> Option<Self::Item> {
    if self.bytes.is_empty() {
      return None;
    }

    let (entry, remaining) = self.bytes.split_at(self.layout.entry_size());

    self.bytes = remaining;

    let (address, size) = entry.split_at(self.layout.address_size());

    let address = decode_validated_cells_u64(address);
    let size = decode_validated_cells_u64(size);

    Some(MemoryRange { address, size })
  }
}

impl<'a> MemoryRanges<'a> {
  /// Creates a memory-range iterator from a semantically validated root node.
  ///
  /// # Panics
  ///
  /// Panics if the root's effective child-addressing information cannot be
  /// represented as a `reg` layout. Memory semantic validation guarantees this
  /// invariant before this constructor is used.
  #[expect(
    clippy::expect_used,
    reason = "Fdt semantic validation guarantees a representable root reg layout"
  )]
  pub(crate) fn new(root: &Node<'a>) -> Self {
    let addressing = addressing::child_addressing(root);

    let layout =
      RegLayout::new(addressing).expect("validated memory addressing must be representable");

    Self {
      nodes: root.children(),
      current: None,
      layout,
    }
  }
}

impl Iterator for MemoryRanges<'_> {
  type Item = MemoryRange;

  fn next(&mut self) -> Option<Self::Item> {
    loop {
      if let Some(ranges) = self.current.as_mut()
        && let Some(range) = ranges.next()
      {
        return Some(range);
      }

      self.current = None;

      let node = self
        .nodes
        .find(|node| node.name_component() == MEMORY_NODE_NAME)?;

      self.current = Some(MemoryNodeRanges::new(&node, self.layout));
    }
  }
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

    addressing::validate(&root)?;
    validate(&root)
  }

  #[test]
  fn memory_ranges_flatten_entries_across_memory_nodes() {
    let mut bytes = Vec::new();

    push_begin_node(&mut bytes, b"");

    push_begin_node(&mut bytes, b"cpus");
    push_end_node(&mut bytes);

    // Two ranges in the first memory node.
    push_begin_node(&mut bytes, b"memory@100000000");
    push_property(&mut bytes, DEVICE_TYPE_OFFSET, b"memory\0");
    push_reg_cells(
      &mut bytes,
      &[
        // address = 0x1_0000_0000, size = 0x1000_0000
        0x0000_0001,
        0x0000_0000,
        0x1000_0000,
        // address = 0x2_0000_0000, size = 0x2000_0000
        0x0000_0002,
        0x0000_0000,
        0x2000_0000,
      ],
    );
    push_end_node(&mut bytes);

    // One range in a second memory node.
    push_begin_node(&mut bytes, b"memory@300000000");
    push_property(&mut bytes, DEVICE_TYPE_OFFSET, b"memory\0");
    push_reg_cells(
      &mut bytes,
      &[
        // address = 0x3_0000_0000, size = 0x3000_0000
        0x0000_0003,
        0x0000_0000,
        0x3000_0000,
      ],
    );
    push_end_node(&mut bytes);

    push_end_node(&mut bytes);
    push_end(&mut bytes);

    let strings = strings();
    let structure =
      Structure::new(&bytes, &strings).expect("test structure should be structurally valid");

    let root = structure.root(strings);

    addressing::validate(&root).expect("test addressing should be valid");
    validate(&root).expect("test memory nodes should be valid");

    let mut ranges = MemoryRanges::new(&root);

    let first = ranges.next().expect("first memory range should exist");
    assert_eq!(first.address(), 0x1_0000_0000);
    assert_eq!(first.size(), 0x1000_0000);

    let second = ranges.next().expect("second memory range should exist");
    assert_eq!(second.address(), 0x2_0000_0000);
    assert_eq!(second.size(), 0x2000_0000);

    let third = ranges.next().expect("third memory range should exist");
    assert_eq!(third.address(), 0x3_0000_0000);
    assert_eq!(third.size(), 0x3000_0000);

    assert!(ranges.next().is_none());
  }

  #[test]
  fn memory_ranges_use_root_cell_counts() {
    let mut bytes = Vec::new();

    push_begin_node(&mut bytes, b"");

    // One address cell and two size cells.
    push_cell_counts(&mut bytes, 1, 2);

    push_begin_node(&mut bytes, b"cpus");
    push_end_node(&mut bytes);

    push_begin_node(&mut bytes, b"memory@12345678");
    push_property(&mut bytes, DEVICE_TYPE_OFFSET, b"memory\0");
    push_reg_cells(
      &mut bytes,
      &[
        0x1234_5678, // address
        0x0000_0001, // size high
        0x8000_0000, // size low
      ],
    );
    push_end_node(&mut bytes);

    push_end_node(&mut bytes);
    push_end(&mut bytes);

    let strings = strings();
    let structure =
      Structure::new(&bytes, &strings).expect("test structure should be structurally valid");

    let root = structure.root(strings);

    addressing::validate(&root).expect("test addressing should be valid");
    validate(&root).expect("test memory node should be valid");

    let mut ranges = MemoryRanges::new(&root);

    let range = ranges.next().expect("memory range should exist");

    assert_eq!(range.address(), 0x1234_5678);
    assert_eq!(range.size(), 0x1_8000_0000);
    assert!(ranges.next().is_none());
  }

  #[test]
  fn memory_node_without_device_type_is_rejected() {
    let mut bytes = Vec::new();

    push_begin_node(&mut bytes, b"");

    push_begin_node(&mut bytes, b"cpus");
    push_end_node(&mut bytes);

    push_begin_node(&mut bytes, b"memory@0");
    push_reg_cells(&mut bytes, &[0, 0, 0x1000]);
    push_end_node(&mut bytes);

    push_end_node(&mut bytes);
    push_end(&mut bytes);

    assert_eq!(
      validate_structure(&bytes),
      Err(SemanticError::MissingMemoryDeviceType)
    );
  }

  #[test]
  fn memory_node_with_invalid_device_type_is_rejected() {
    let mut bytes = Vec::new();

    push_begin_node(&mut bytes, b"");

    push_begin_node(&mut bytes, b"cpus");
    push_end_node(&mut bytes);

    push_begin_node(&mut bytes, b"memory@0");
    push_property(&mut bytes, DEVICE_TYPE_OFFSET, b"cpu\0");
    push_reg_cells(&mut bytes, &[0, 0, 0x1000]);
    push_end_node(&mut bytes);

    push_end_node(&mut bytes);
    push_end(&mut bytes);

    assert_eq!(
      validate_structure(&bytes),
      Err(SemanticError::InvalidMemoryDeviceType)
    );
  }

  #[test]
  fn memory_node_without_reg_is_rejected() {
    let mut bytes = Vec::new();

    push_begin_node(&mut bytes, b"");

    push_begin_node(&mut bytes, b"cpus");
    push_end_node(&mut bytes);

    // Deliberately no unit address here. A memory@0 node without reg would
    // already be rejected by generic addressing validation.
    push_begin_node(&mut bytes, b"memory");
    push_property(&mut bytes, DEVICE_TYPE_OFFSET, b"memory\0");
    push_end_node(&mut bytes);

    push_end_node(&mut bytes);
    push_end(&mut bytes);

    assert_eq!(
      validate_structure(&bytes),
      Err(SemanticError::MissingMemoryReg)
    );
  }

  #[test]
  fn memory_address_larger_than_u64_is_rejected() {
    let mut bytes = Vec::new();

    push_begin_node(&mut bytes, b"");

    // Three cells for address, one for size.
    push_cell_counts(&mut bytes, 3, 1);

    push_begin_node(&mut bytes, b"cpus");
    push_end_node(&mut bytes);

    push_begin_node(&mut bytes, b"memory");
    push_property(&mut bytes, DEVICE_TYPE_OFFSET, b"memory\0");

    push_reg_cells(
      &mut bytes,
      &[
        // 0x1_00000000_00000000 = 2^64
        0x0000_0001,
        0x0000_0000,
        0x0000_0000,
        // size
        0x0000_1000,
      ],
    );

    push_end_node(&mut bytes);

    push_end_node(&mut bytes);
    push_end(&mut bytes);

    assert_eq!(
      validate_structure(&bytes),
      Err(SemanticError::MemoryAddressDoesNotFitU64)
    );
  }

  #[test]
  fn memory_size_larger_than_u64_is_rejected() {
    let mut bytes = Vec::new();

    push_begin_node(&mut bytes, b"");

    // One address cell, three size cells.
    push_cell_counts(&mut bytes, 1, 3);

    push_begin_node(&mut bytes, b"cpus");
    push_end_node(&mut bytes);

    push_begin_node(&mut bytes, b"memory");
    push_property(&mut bytes, DEVICE_TYPE_OFFSET, b"memory\0");

    push_reg_cells(
      &mut bytes,
      &[
        // address
        0x0000_0000,
        // size = 0x1_00000000_00000000 = 2^64
        0x0000_0001,
        0x0000_0000,
        0x0000_0000,
      ],
    );

    push_end_node(&mut bytes);

    push_end_node(&mut bytes);
    push_end(&mut bytes);

    assert_eq!(
      validate_structure(&bytes),
      Err(SemanticError::MemorySizeDoesNotFitU64)
    );
  }

  #[test]
  fn memory_address_with_extra_zero_cells_is_accepted() {
    let mut bytes = Vec::new();

    push_begin_node(&mut bytes, b"");

    // More than two address cells is allowed as long as the represented
    // value itself still fits in u64.
    push_cell_counts(&mut bytes, 3, 1);

    push_begin_node(&mut bytes, b"cpus");
    push_end_node(&mut bytes);

    push_begin_node(&mut bytes, b"memory");
    push_property(&mut bytes, DEVICE_TYPE_OFFSET, b"memory\0");

    push_reg_cells(
      &mut bytes,
      &[
        // Three address cells, but the high one is zero:
        // 0x00000000_00000001_80000000
        0x0000_0000,
        0x0000_0001,
        0x8000_0000,
        // size
        0x0000_1000,
      ],
    );

    push_end_node(&mut bytes);

    push_end_node(&mut bytes);
    push_end(&mut bytes);

    let strings = strings();
    let structure =
      Structure::new(&bytes, &strings).expect("test structure should be structurally valid");

    let root = structure.root(strings);

    addressing::validate(&root).expect("test addressing should be valid");
    validate(&root).expect("memory value should fit in u64");

    let mut ranges = MemoryRanges::new(&root);
    let range = ranges.next().expect("memory range should exist");

    assert_eq!(range.address(), 0x1_8000_0000);
    assert_eq!(range.size(), 0x1000);
    assert!(ranges.next().is_none());
  }

  #[test]
  fn zero_memory_address_cells_are_rejected() {
    let mut bytes = Vec::new();

    push_begin_node(&mut bytes, b"");

    push_cell_counts(&mut bytes, 0, 1);

    push_begin_node(&mut bytes, b"cpus");
    push_end_node(&mut bytes);

    push_begin_node(&mut bytes, b"memory");
    push_property(&mut bytes, DEVICE_TYPE_OFFSET, b"memory\0");

    // With (0, 1), one reg entry consists only of one size cell.
    push_reg_cells(&mut bytes, &[0x1000]);

    push_end_node(&mut bytes);

    push_end_node(&mut bytes);
    push_end(&mut bytes);

    assert_eq!(
      validate_structure(&bytes),
      Err(SemanticError::InvalidMemoryAddressCells)
    );
  }

  #[test]
  fn zero_memory_size_cells_are_rejected() {
    let mut bytes = Vec::new();

    push_begin_node(&mut bytes, b"");

    push_cell_counts(&mut bytes, 1, 0);

    push_begin_node(&mut bytes, b"cpus");
    push_end_node(&mut bytes);

    push_begin_node(&mut bytes, b"memory");
    push_property(&mut bytes, DEVICE_TYPE_OFFSET, b"memory\0");

    // With (1, 0), one reg entry consists only of one address cell.
    push_reg_cells(&mut bytes, &[0x8000_0000]);

    push_end_node(&mut bytes);

    push_end_node(&mut bytes);
    push_end(&mut bytes);

    assert_eq!(
      validate_structure(&bytes),
      Err(SemanticError::InvalidMemorySizeCells)
    );
  }

  #[test]
  fn empty_memory_reg_is_rejected() {
    let mut bytes = Vec::new();

    push_begin_node(&mut bytes, b"");

    push_begin_node(&mut bytes, b"cpus");
    push_end_node(&mut bytes);

    push_begin_node(&mut bytes, b"memory@0");
    push_property(&mut bytes, DEVICE_TYPE_OFFSET, b"memory\0");

    // Present but contains no address-size pairs.
    push_reg_cells(&mut bytes, &[]);

    push_end_node(&mut bytes);

    push_end_node(&mut bytes);
    push_end(&mut bytes);

    assert_eq!(
      validate_structure(&bytes),
      Err(SemanticError::EmptyMemoryReg)
    );
  }

  #[test]
  fn zero_sized_memory_range_is_rejected() {
    let mut bytes = Vec::new();

    push_begin_node(&mut bytes, b"");

    push_begin_node(&mut bytes, b"cpus");
    push_end_node(&mut bytes);

    push_begin_node(&mut bytes, b"memory@80000000");
    push_property(&mut bytes, DEVICE_TYPE_OFFSET, b"memory\0");

    push_reg_cells(
      &mut bytes,
      &[
        0x0000_0000,
        0x8000_0000, // address = 0x8000_0000
        0x0000_0000, // size = 0
      ],
    );

    push_end_node(&mut bytes);

    push_end_node(&mut bytes);
    push_end(&mut bytes);

    assert_eq!(
      validate_structure(&bytes),
      Err(SemanticError::ZeroMemorySize)
    );
  }
}
