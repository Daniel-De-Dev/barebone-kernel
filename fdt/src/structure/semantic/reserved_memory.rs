//! Validation and interpretation of static `/reserved-memory` regions.
//!
//! This implementation exposes static allocations described by child `reg`
//! properties. Dynamic allocations are currently unsupported.
//!
//! A successfully validated tree can be traversed infallibly through
//! [`ReservedMemoryRanges`].

use core::fmt;

use crate::structure::{Children, Node};

use super::{
  SemanticError,
  addressing::{self, RegLayout},
  reg_range::{self, RegRangeError, RegRanges},
};

/// Exact name of the optional reserved-memory node that may appear as a direct
/// child of the Devicetree root.
const RESERVED_MEMORY_NODE_NAME: &[u8] = b"reserved-memory";

/// Name of the required property specifying the number of address cells used
/// by direct children of `/reserved-memory`.
const ADDRESS_CELLS_PROPERTY: &[u8] = b"#address-cells";

/// Name of the required property specifying the number of size cells used by
/// direct children of `/reserved-memory`.
const SIZE_CELLS_PROPERTY: &[u8] = b"#size-cells";

/// Name of the required address-translation property on `/reserved-memory`.
const RANGES_PROPERTY: &[u8] = b"ranges";

/// Name of the property describing one or more statically allocated reserved
/// address-size ranges.
const REG_PROPERTY: &[u8] = b"reg";

/// Name of the property requesting the size of a dynamically allocated
/// reserved-memory region.
const SIZE_PROPERTY: &[u8] = b"size";

/// A statically allocated physical range described by a direct child of
/// `/reserved-memory`.
///
/// The range contains the physical start address and byte size encoded by the
/// child's `reg` property.
#[derive(Clone, Copy)]
pub struct ReservedMemoryRange {
  /// Physical start address of the reserved range.
  address: u64,

  /// Size of the reserved range in bytes.
  size: u64,
}

impl fmt::Debug for ReservedMemoryRange {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.debug_struct("ReservedMemoryRange")
      .field("address", &format_args!("{:#x}", self.address))
      .field("size", &format_args!("{:#x}", self.size))
      .finish()
  }
}

/// Iterator over static physical ranges described under `/reserved-memory`.
///
/// Ranges from multiple children and multiple `reg` entries are flattened into
/// one sequence. The iterator must be constructed from a tree accepted by
/// [`validate`].
///
/// If `/reserved-memory` is absent, the iterator is empty.
pub struct ReservedMemoryRanges<'a> {
  /// Remaining direct children of `/reserved-memory` to inspect.
  ///
  /// `None` means the Devicetree contains no `/reserved-memory` node.
  nodes: Option<Children<'a>>,

  /// Range iterator for the static reserved-memory child currently being
  /// consumed.
  current: Option<RegRanges<'a>>,

  /// Layout used to decode `reg` entries belonging to `/reserved-memory`
  /// children.
  ///
  /// This is `None` exactly when no `/reserved-memory` node was found.
  layout: Option<RegLayout>,
}

impl ReservedMemoryRange {
  /// Returns the physical start address of the reserved range.
  #[must_use]
  pub const fn address(self) -> u64 {
    self.address
  }

  /// Returns the size of the reserved range in bytes.
  #[must_use]
  pub const fn size(self) -> u64 {
    self.size
  }
}

/// Finds the exact `/reserved-memory` node among the root's direct children.
///
/// Only a child whose complete node name is `reserved-memory` is matched;
/// unit-address variants or nested nodes are not considered.
///
/// Returns `None` when the root has no such direct child.
fn reserved_memory_node<'a>(root: &Node<'a>) -> Option<Node<'a>> {
  root
    .children()
    .find(|node| node.name() == RESERVED_MEMORY_NODE_NAME)
}

/// Validates the required properties of `/reserved-memory`.
///
/// # Errors
///
/// Returns [`SemanticError::MissingReservedMemoryAddressCells`] if
/// `#address-cells` is absent.
///
/// Returns [`SemanticError::MissingReservedMemorySizeCells`] if `#size-cells`
/// is absent.
///
/// Returns [`SemanticError::MissingReservedMemoryRanges`] if `ranges` is
/// absent.
///
/// Returns [`SemanticError::NonEmptyReservedMemoryRanges`] if `ranges` is not
/// empty.
fn validate_parent(node: &Node<'_>) -> Result<(), SemanticError> {
  if node.property(ADDRESS_CELLS_PROPERTY).is_none() {
    return Err(SemanticError::MissingReservedMemoryAddressCells);
  }

  if node.property(SIZE_CELLS_PROPERTY).is_none() {
    return Err(SemanticError::MissingReservedMemorySizeCells);
  }

  let Some(ranges) = node.property(RANGES_PROPERTY) else {
    return Err(SemanticError::MissingReservedMemoryRanges);
  };

  if !ranges.value().is_empty() {
    return Err(SemanticError::NonEmptyReservedMemoryRanges);
  }

  Ok(())
}

/// Validates the supported `/reserved-memory` semantics rooted at `root`.
///
/// Absence of `/reserved-memory` is valid.
///
/// # Errors
///
/// Returns the first [`SemanticError`] encountered while validating the
/// `/reserved-memory` parent, its addressing layout, or one of its children.
pub(super) fn validate(root: &Node<'_>) -> Result<(), SemanticError> {
  let Some(reserved_memory) = reserved_memory_node(root) else {
    return Ok(());
  };

  validate_parent(&reserved_memory)?;

  let addressing = addressing::child_addressing(&reserved_memory);

  if addressing.address_cells().get() == 0 {
    return Err(SemanticError::InvalidReservedMemoryAddressCells);
  }

  if addressing.size_cells().get() == 0 {
    return Err(SemanticError::InvalidReservedMemorySizeCells);
  }

  let layout = RegLayout::new(addressing)?;

  for child in reserved_memory.children() {
    validate_child(&child, layout)?;
  }

  Ok(())
}

/// Validates the allocation form of one `/reserved-memory` child.
///
/// A present `reg` takes precedence over `size`.
///
/// # Errors
///
/// Returns [`SemanticError::UnsupportedDynamicReservedMemory`] if only `size`
/// describes the allocation.
///
/// Returns [`SemanticError::MissingReservedMemoryAllocation`] if neither
/// allocation property is present.
///
/// Propagates errors from static `reg` validation.
fn validate_child(node: &Node<'_>, layout: RegLayout) -> Result<(), SemanticError> {
  if let Some(reg) = node.property(REG_PROPERTY) {
    return validate_reg(reg.value(), layout);
  }

  if node.property(SIZE_PROPERTY).is_some() {
    return Err(SemanticError::UnsupportedDynamicReservedMemory);
  }

  Err(SemanticError::MissingReservedMemoryAllocation)
}

/// Validates a static `/reserved-memory` `reg` value.
///
/// Generic range-validation failures are translated into their corresponding
/// reserved-memory semantic errors.
///
/// # Errors
///
/// Returns the [`SemanticError`] corresponding to the first invalid encoded
/// range.
fn validate_reg(bytes: &[u8], layout: RegLayout) -> Result<(), SemanticError> {
  reg_range::validate(bytes, layout).map_err(|error| match error {
    RegRangeError::Empty => SemanticError::EmptyReservedMemoryReg,
    RegRangeError::AddressDoesNotFitU64 => SemanticError::ReservedMemoryAddressDoesNotFitU64,
    RegRangeError::SizeDoesNotFitU64 => SemanticError::ReservedMemorySizeDoesNotFitU64,
    RegRangeError::ZeroSize => SemanticError::ZeroReservedMemorySize,
    RegRangeError::EndOverflow => SemanticError::ReservedMemoryRangeEndOverflow,
  })
}

impl<'a> ReservedMemoryRanges<'a> {
  /// Creates an iterator from a root previously accepted by [`validate`].
  ///
  /// If `/reserved-memory` is absent, the returned iterator is empty.
  ///
  /// # Panics
  ///
  /// Panics if the addressing information of a present `/reserved-memory` node
  /// does not satisfy the invariants established by [`validate`].
  pub(crate) fn new(root: &Node<'a>) -> Self {
    let Some(reserved_memory) = reserved_memory_node(root) else {
      return Self {
        nodes: None,
        current: None,
        layout: None,
      };
    };

    let addressing = addressing::child_addressing(&reserved_memory);

    #[expect(
      clippy::expect_used,
      reason = "reserved-memory semantic validation guarantees a representable reg layout"
    )]
    let layout = RegLayout::new(addressing)
      .expect("validated reserved-memory addressing must be representable");

    Self {
      nodes: Some(reserved_memory.children()),
      current: None,
      layout: Some(layout),
    }
  }
}

impl Iterator for ReservedMemoryRanges<'_> {
  type Item = ReservedMemoryRange;

  #[expect(
    clippy::expect_used,
    reason = "reserved-memory semantic validation guarantees static children contain reg"
  )]
  fn next(&mut self) -> Option<Self::Item> {
    loop {
      if let Some(ranges) = self.current.as_mut()
        && let Some(range) = ranges.next()
      {
        return Some(ReservedMemoryRange {
          address: range.address(),
          size: range.size(),
        });
      }

      self.current = None;

      let node = self.nodes.as_mut()?.next()?;

      let reg = node
        .property(REG_PROPERTY)
        .expect("validated static reserved-memory child must contain reg");

      let layout = self
        .layout
        .expect("present reserved-memory node must have a validated layout");

      self.current = Some(RegRanges::new(reg.value(), layout));
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

  fn push_reserved_memory_begin(bytes: &mut Vec<u8>, address_cells: u32, size_cells: u32) {
    push_begin_node(bytes, RESERVED_MEMORY_NODE_NAME);
    push_cell_counts(bytes, address_cells, size_cells);
    push_property(bytes, RANGES_OFFSET, &[]);
  }

  #[test]
  fn reserved_memory_range_debug_uses_hex() {
    let range = ReservedMemoryRange {
      address: 0x8000_0000,
      size: 0x20_0000,
    };

    assert_eq!(
      std::format!("{range:?}"),
      "ReservedMemoryRange { address: 0x80000000, size: 0x200000 }"
    );
  }

  #[test]
  fn absent_reserved_memory_node_yields_no_ranges() {
    let mut bytes = Vec::new();

    push_begin_node(&mut bytes, b"");
    push_end_node(&mut bytes);
    push_end(&mut bytes);

    let strings = strings();
    let structure =
      Structure::new(&bytes, &strings).expect("test structure should be structurally valid");

    let root = structure.root(strings);

    addressing::validate(&root).expect("test addressing should be valid");
    validate(&root).expect("absence of reserved-memory should be valid");

    let mut ranges = ReservedMemoryRanges::new(&root);

    assert!(ranges.next().is_none());
  }

  #[test]
  fn reserved_memory_ranges_flatten_entries_across_children() {
    let mut bytes = Vec::new();

    push_begin_node(&mut bytes, b"");

    push_reserved_memory_begin(&mut bytes, 2, 1);

    // Two ranges in the first child.
    push_begin_node(&mut bytes, b"first@100000000");
    push_reg_cells(
      &mut bytes,
      &[
        // address = 0x1_0000_0000, size = 0x1000
        0x0000_0001,
        0x0000_0000,
        0x0000_1000,
        // address = 0x2_0000_0000, size = 0x2000
        0x0000_0002,
        0x0000_0000,
        0x0000_2000,
      ],
    );
    push_end_node(&mut bytes);

    // One range in the second child.
    push_begin_node(&mut bytes, b"second@300000000");
    push_reg_cells(&mut bytes, &[0x0000_0003, 0x0000_0000, 0x0000_3000]);
    push_end_node(&mut bytes);

    push_end_node(&mut bytes); // reserved-memory
    push_end_node(&mut bytes); // root
    push_end(&mut bytes);

    let strings = strings();
    let structure =
      Structure::new(&bytes, &strings).expect("test structure should be structurally valid");

    let root = structure.root(strings);

    addressing::validate(&root).expect("test addressing should be valid");
    validate(&root).expect("test reserved-memory should be valid");

    let mut ranges = ReservedMemoryRanges::new(&root);

    let first = ranges.next().expect("first range should exist");
    assert_eq!(first.address(), 0x1_0000_0000);
    assert_eq!(first.size(), 0x1000);

    let second = ranges.next().expect("second range should exist");
    assert_eq!(second.address(), 0x2_0000_0000);
    assert_eq!(second.size(), 0x2000);

    let third = ranges.next().expect("third range should exist");
    assert_eq!(third.address(), 0x3_0000_0000);
    assert_eq!(third.size(), 0x3000);

    assert!(ranges.next().is_none());
  }

  #[test]
  fn reserved_memory_ranges_use_reserved_memory_cell_counts() {
    let mut bytes = Vec::new();

    push_begin_node(&mut bytes, b"");

    // Deliberately different from /reserved-memory.
    push_cell_counts(&mut bytes, 2, 1);

    // One address cell and two size cells.
    push_reserved_memory_begin(&mut bytes, 1, 2);

    push_begin_node(&mut bytes, b"buffer@12345678");
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
    push_end_node(&mut bytes);
    push_end(&mut bytes);

    let strings = strings();
    let structure =
      Structure::new(&bytes, &strings).expect("test structure should be structurally valid");

    let root = structure.root(strings);

    addressing::validate(&root).expect("test addressing should be valid");
    validate(&root).expect("test reserved-memory should be valid");

    let mut ranges = ReservedMemoryRanges::new(&root);

    let range = ranges.next().expect("reserved-memory range should exist");

    assert_eq!(range.address(), 0x1234_5678);
    assert_eq!(range.size(), 0x1_8000_0000);
    assert!(ranges.next().is_none());
  }

  #[test]
  fn missing_reserved_memory_address_cells_are_rejected() {
    let mut bytes = Vec::new();

    push_begin_node(&mut bytes, b"");

    push_begin_node(&mut bytes, RESERVED_MEMORY_NODE_NAME);
    push_property(&mut bytes, SIZE_CELLS_OFFSET, &1_u32.to_be_bytes());
    push_property(&mut bytes, RANGES_OFFSET, &[]);
    push_end_node(&mut bytes);

    push_end_node(&mut bytes);
    push_end(&mut bytes);

    assert_eq!(
      validate_structure(&bytes),
      Err(SemanticError::MissingReservedMemoryAddressCells)
    );
  }

  #[test]
  fn missing_reserved_memory_size_cells_are_rejected() {
    let mut bytes = Vec::new();

    push_begin_node(&mut bytes, b"");

    push_begin_node(&mut bytes, RESERVED_MEMORY_NODE_NAME);
    push_property(&mut bytes, ADDRESS_CELLS_OFFSET, &2_u32.to_be_bytes());
    push_property(&mut bytes, RANGES_OFFSET, &[]);
    push_end_node(&mut bytes);

    push_end_node(&mut bytes);
    push_end(&mut bytes);

    assert_eq!(
      validate_structure(&bytes),
      Err(SemanticError::MissingReservedMemorySizeCells)
    );
  }

  #[test]
  fn missing_reserved_memory_ranges_are_rejected() {
    let mut bytes = Vec::new();

    push_begin_node(&mut bytes, b"");

    push_begin_node(&mut bytes, RESERVED_MEMORY_NODE_NAME);
    push_cell_counts(&mut bytes, 2, 1);
    push_end_node(&mut bytes);

    push_end_node(&mut bytes);
    push_end(&mut bytes);

    assert_eq!(
      validate_structure(&bytes),
      Err(SemanticError::MissingReservedMemoryRanges)
    );
  }

  #[test]
  fn nonempty_reserved_memory_ranges_are_rejected() {
    let mut bytes = Vec::new();

    push_begin_node(&mut bytes, b"");

    push_begin_node(&mut bytes, RESERVED_MEMORY_NODE_NAME);
    push_cell_counts(&mut bytes, 2, 1);
    push_property(&mut bytes, RANGES_OFFSET, &[0, 0, 0, 0]);
    push_end_node(&mut bytes);

    push_end_node(&mut bytes);
    push_end(&mut bytes);

    assert_eq!(
      validate_structure(&bytes),
      Err(SemanticError::NonEmptyReservedMemoryRanges)
    );
  }

  #[test]
  fn zero_reserved_memory_address_cells_are_rejected() {
    let mut bytes = Vec::new();

    push_begin_node(&mut bytes, b"");
    push_reserved_memory_begin(&mut bytes, 0, 1);
    push_end_node(&mut bytes);
    push_end_node(&mut bytes);
    push_end(&mut bytes);

    assert_eq!(
      validate_structure(&bytes),
      Err(SemanticError::InvalidReservedMemoryAddressCells)
    );
  }

  #[test]
  fn zero_reserved_memory_size_cells_are_rejected() {
    let mut bytes = Vec::new();

    push_begin_node(&mut bytes, b"");
    push_reserved_memory_begin(&mut bytes, 1, 0);
    push_end_node(&mut bytes);
    push_end_node(&mut bytes);
    push_end(&mut bytes);

    assert_eq!(
      validate_structure(&bytes),
      Err(SemanticError::InvalidReservedMemorySizeCells)
    );
  }

  #[test]
  fn reserved_memory_child_without_allocation_is_rejected() {
    let mut bytes = Vec::new();

    push_begin_node(&mut bytes, b"");
    push_reserved_memory_begin(&mut bytes, 2, 1);

    // No unit address so generic addressing validation does not reject this
    // before reserved-memory semantic validation gets to inspect it.
    push_begin_node(&mut bytes, b"buffer");
    push_end_node(&mut bytes);

    push_end_node(&mut bytes);
    push_end_node(&mut bytes);
    push_end(&mut bytes);

    assert_eq!(
      validate_structure(&bytes),
      Err(SemanticError::MissingReservedMemoryAllocation)
    );
  }

  #[test]
  fn dynamic_reserved_memory_is_rejected_as_unsupported() {
    let mut bytes = Vec::new();

    push_begin_node(&mut bytes, b"");
    push_reserved_memory_begin(&mut bytes, 2, 1);

    push_begin_node(&mut bytes, b"buffer");
    push_property(&mut bytes, SIZE_OFFSET, &0x1000_u32.to_be_bytes());
    push_end_node(&mut bytes);

    push_end_node(&mut bytes);
    push_end_node(&mut bytes);
    push_end(&mut bytes);

    assert_eq!(
      validate_structure(&bytes),
      Err(SemanticError::UnsupportedDynamicReservedMemory)
    );
  }

  #[test]
  fn reg_takes_precedence_over_size() {
    let mut bytes = Vec::new();

    push_begin_node(&mut bytes, b"");
    push_reserved_memory_begin(&mut bytes, 2, 1);

    push_begin_node(&mut bytes, b"buffer@80000000");
    push_reg_cells(&mut bytes, &[0x0000_0000, 0x8000_0000, 0x0000_1000]);
    push_property(&mut bytes, SIZE_OFFSET, &0x2000_u32.to_be_bytes());
    push_end_node(&mut bytes);

    push_end_node(&mut bytes);
    push_end_node(&mut bytes);
    push_end(&mut bytes);

    let strings = strings();
    let structure =
      Structure::new(&bytes, &strings).expect("test structure should be structurally valid");

    let root = structure.root(strings);

    addressing::validate(&root).expect("test addressing should be valid");
    validate(&root).expect("reg should take precedence over size");

    let mut ranges = ReservedMemoryRanges::new(&root);
    let range = ranges.next().expect("static range should exist");

    assert_eq!(range.address(), 0x8000_0000);
    assert_eq!(range.size(), 0x1000);
    assert!(ranges.next().is_none());
  }

  #[test]
  fn empty_reserved_memory_reg_is_rejected() {
    let mut bytes = Vec::new();

    push_begin_node(&mut bytes, b"");
    push_reserved_memory_begin(&mut bytes, 2, 1);

    push_begin_node(&mut bytes, b"buffer");
    push_reg_cells(&mut bytes, &[]);
    push_end_node(&mut bytes);

    push_end_node(&mut bytes);
    push_end_node(&mut bytes);
    push_end(&mut bytes);

    assert_eq!(
      validate_structure(&bytes),
      Err(SemanticError::EmptyReservedMemoryReg)
    );
  }

  #[test]
  fn reserved_memory_address_larger_than_u64_is_rejected() {
    let mut bytes = Vec::new();

    push_begin_node(&mut bytes, b"");
    push_reserved_memory_begin(&mut bytes, 3, 1);

    push_begin_node(&mut bytes, b"buffer");
    push_reg_cells(
      &mut bytes,
      &[
        // address = 2^64
        0x0000_0001,
        0x0000_0000,
        0x0000_0000,
        // size
        0x0000_1000,
      ],
    );
    push_end_node(&mut bytes);

    push_end_node(&mut bytes);
    push_end_node(&mut bytes);
    push_end(&mut bytes);

    assert_eq!(
      validate_structure(&bytes),
      Err(SemanticError::ReservedMemoryAddressDoesNotFitU64)
    );
  }

  #[test]
  fn reserved_memory_size_larger_than_u64_is_rejected() {
    let mut bytes = Vec::new();

    push_begin_node(&mut bytes, b"");
    push_reserved_memory_begin(&mut bytes, 1, 3);

    push_begin_node(&mut bytes, b"buffer");
    push_reg_cells(
      &mut bytes,
      &[
        // address
        0x0000_0000,
        // size = 2^64
        0x0000_0001,
        0x0000_0000,
        0x0000_0000,
      ],
    );
    push_end_node(&mut bytes);

    push_end_node(&mut bytes);
    push_end_node(&mut bytes);
    push_end(&mut bytes);

    assert_eq!(
      validate_structure(&bytes),
      Err(SemanticError::ReservedMemorySizeDoesNotFitU64)
    );
  }

  #[test]
  fn zero_sized_reserved_memory_range_is_rejected() {
    let mut bytes = Vec::new();

    push_begin_node(&mut bytes, b"");
    push_reserved_memory_begin(&mut bytes, 2, 1);

    push_begin_node(&mut bytes, b"buffer");
    push_reg_cells(&mut bytes, &[0x0000_0000, 0x8000_0000, 0x0000_0000]);
    push_end_node(&mut bytes);

    push_end_node(&mut bytes);
    push_end_node(&mut bytes);
    push_end(&mut bytes);

    assert_eq!(
      validate_structure(&bytes),
      Err(SemanticError::ZeroReservedMemorySize)
    );
  }

  #[test]
  fn reserved_memory_range_end_overflow_is_rejected() {
    let mut bytes = Vec::new();

    push_begin_node(&mut bytes, b"");

    push_reserved_memory_begin(&mut bytes, 2, 1);

    push_begin_node(&mut bytes, b"buffer@ffffffffffffffff");
    push_reg_cells(
      &mut bytes,
      &[
        // address = u64::MAX
        0xffff_ffff,
        0xffff_ffff,
        // size = 1
        0x0000_0001,
      ],
    );
    push_end_node(&mut bytes);

    push_end_node(&mut bytes); // reserved-memory
    push_end_node(&mut bytes); // root
    push_end(&mut bytes);

    assert_eq!(
      validate_structure(&bytes),
      Err(SemanticError::ReservedMemoryRangeEndOverflow)
    );
  }
}
