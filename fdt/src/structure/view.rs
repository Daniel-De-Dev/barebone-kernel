//! Read-only navigation over a validated FDT structure block.
//!
//! The views in this module operate on structure blocks that have passed
//! [`Structure::new`]. Traversal therefore relies on the representation-level
//! invariants established there and treats violations of those invariants as
//! internal errors rather than malformed input.

use crate::{helpers::usize_from_u32, reader::Reader, strings::Strings};

use super::{
  Structure, Token,
  name::{component, unit_address},
  read_token,
};

/// A sequential reader over an already validated structure block.
///
/// This wraps [`Reader`] and converts operations that are fallible for
/// arbitrary input into infallible operations under the invariants established
/// by [`Structure::new`].
///
/// Any failure of an underlying [`Reader`] operation indicates an internal
/// violation of those invariants rather than malformed external input.
struct ValidatedReader<'a> {
  /// Underlying bounded reader over validated structure-block bytes.
  inner: Reader<'a>,
}

/// A read-only view of one node in a validated devicetree.
pub struct Node<'a> {
  /// Full node name, including a unit address when present.
  name: &'a [u8],

  /// Structure-block bytes beginning immediately after the padded
  /// node name.
  ///
  /// This slice may extend beyond the end of this node. Structure
  /// tokens are used while traversing to determine where the node ends.
  content: &'a [u8],

  /// Strings block used to resolve property names.
  strings: Strings<'a>,
}

/// A read-only view of one property in a validated devicetree node.
pub struct Property<'a> {
  /// Property name resolved from the validated strings block.
  name: &'a [u8],

  /// Raw, uninterpreted property value bytes.
  value: &'a [u8],
}

/// Iterator over the properties belonging directly to a node.
pub struct Properties<'a> {
  /// Reader positioned within the contents of the node being iterated.
  reader: ValidatedReader<'a>,

  /// Strings block used to resolve property-name offsets.
  strings: Strings<'a>,

  /// Whether the end of the node's direct property sequence has been reached.
  finished: bool,
}

/// Iterator over the direct children of a node.
pub struct Children<'a> {
  /// Reader positioned within the contents of the node being iterated.
  reader: ValidatedReader<'a>,

  /// Strings block propagated to child-node views.
  strings: Strings<'a>,

  /// Current nesting depth relative to the node being iterated.
  ///
  /// A depth of zero means traversal is within the direct contents of the
  /// node. A depth of one means traversal is inside a direct child, and larger
  /// values represent deeper descendants.
  depth: usize,

  /// Whether the end of the node being iterated has been reached.
  finished: bool,
}

/// Iterator over all descendant nodes of a node.
pub struct Descendants<'a> {
  /// Reader positioned within the contents of the node being iterated.
  reader: ValidatedReader<'a>,

  /// Strings block propagated to child-node views.
  strings: Strings<'a>,

  /// Current nesting depth relative to the node being iterated.
  ///
  /// A depth of zero means traversal is within the direct contents of the
  /// node. A depth of one means traversal is inside a direct child, and larger
  /// values represent deeper descendants.
  depth: usize,

  /// Whether the end of the node being iterated has been reached.
  finished: bool,
}

/// Extracts a value whose success is guaranteed by the validated-structure
/// invariant.
///
/// # Panics
///
/// Panics if `result` is an error, which indicates an internal violation of
/// the invariants established by `Structure::new`, rather than malformed
/// external DTB data.
#[track_caller]
#[expect(
  clippy::expect_used,
  reason = "validated structure operations are infallible unless the view implementation violates Structure invariants"
)]
fn validated<T, E: core::fmt::Debug>(result: Result<T, E>, message: &'static str) -> T {
  result.expect(message)
}

impl<'a> ValidatedReader<'a> {
  /// Creates a reader over bytes belonging to an already validated structure
  /// block.
  const fn new(bytes: &'a [u8]) -> Self {
    Self {
      inner: Reader::new(bytes),
    }
  }

  /// An infallible validated counterpart to [`Reader::read_u32`].
  fn read_u32(&mut self) -> u32 {
    validated(
      self.inner.read_u32(),
      "validated structure remains readable",
    )
  }

  /// An infallible validated counterpart to [`Reader::read_bytes`].
  fn read_bytes(&mut self, length: usize) -> &'a [u8] {
    validated(
      self.inner.read_bytes(length),
      "validated structure remains readable",
    )
  }

  /// An infallible validated counterpart to [`Reader::read_nul_terminated`].
  fn read_nul_terminated(&mut self) -> &'a [u8] {
    validated(
      self.inner.read_nul_terminated(),
      "validated node name remains readable",
    )
  }

  /// An infallible validated counterpart to [`Reader::align_to_4`].
  fn align_to_4(&mut self) {
    validated(
      self.inner.align_to_4(),
      "validated structure contains required alignment",
    );
  }

  /// Returns the unread bytes, equivalent to [`Reader::remaining_bytes`].
  fn remaining_bytes(&self) -> &'a [u8] {
    self.inner.remaining_bytes()
  }

  /// An infallible validated counterpart to [`read_token`].
  fn read_token(&mut self) -> Token {
    validated(
      read_token(&mut self.inner),
      "validated structure contains a valid token",
    )
    .1
  }

  /// Reads and returns the next non-NOP structure-block token.
  fn read_non_nop_token(&mut self) -> Token {
    loop {
      let token = self.read_token();

      if token != Token::Nop {
        return token;
      }
    }
  }

  /// Reads a node name and consumes its required alignment padding.
  ///
  /// The corresponding `FDT_BEGIN_NODE` token must already have been consumed.
  fn read_node_name(&mut self) -> &'a [u8] {
    let name = self.read_nul_terminated();
    self.align_to_4();

    name
  }

  /// Consumes the encoded contents and alignment padding of a property.
  ///
  /// The corresponding `FDT_PROP` token must already have been consumed.
  fn skip_property(&mut self) {
    let length = usize_from_u32(self.read_u32());

    // nameoff
    self.read_u32();

    self.read_bytes(length);
    self.align_to_4();
  }

  /// Reads a property from the validated structure and resolves its name
  /// through `strings`.
  ///
  /// The corresponding `FDT_PROP` token must already have been consumed.
  fn read_property(&mut self, strings: Strings<'a>) -> Property<'a> {
    let length = usize_from_u32(self.read_u32());
    let name_offset = self.read_u32();

    // TODO: Look into defining and using an infallible "read property name"
    let name = validated(
      strings.validate_property_name(name_offset),
      "validated property references a validated property name",
    );

    let value = self.read_bytes(length);

    self.align_to_4();

    Property { name, value }
  }
}

impl<'a> Node<'a> {
  /// Creates a node view from a validated node name and the structure bytes that
  /// follow its padded name.
  ///
  /// `content` may extend beyond this node's closing `FDT_END_NODE`; traversal
  /// determines the node boundary from the validated token structure.
  ///
  /// This constructor must only be used with bytes originating from a validated
  /// [`Structure`].
  const fn new(name: &'a [u8], content: &'a [u8], strings: Strings<'a>) -> Self {
    Self {
      name,
      content,
      strings,
    }
  }

  /// Returns the full node name.
  #[must_use]
  pub const fn name(&self) -> &'a [u8] {
    self.name
  }

  /// Returns the node-name component without its unit address.
  #[must_use]
  pub fn name_component(&self) -> &'a [u8] {
    component(self.name)
  }

  /// Returns the unit-address component when present.
  #[must_use]
  pub fn unit_address(&self) -> Option<&'a [u8]> {
    unit_address(self.name)
  }

  /// Iterates over properties belonging directly to this node.
  #[must_use]
  pub const fn properties(&self) -> Properties<'a> {
    Properties::new(self.content, self.strings)
  }

  /// Finds a property belonging directly to this node whose name exactly
  /// matches `name`.
  #[must_use]
  pub fn property(&self, name: &[u8]) -> Option<Property<'a>> {
    self.properties().find(|property| property.name() == name)
  }

  /// Iterates over child nodes belonging directly to this node.
  #[must_use]
  pub const fn children(&self) -> Children<'a> {
    Children::new(self.content, self.strings)
  }

  /// Finds a direct child whose full node name exactly matches `name`.
  #[must_use]
  pub fn child(&self, name: &[u8]) -> Option<Self> {
    self.children().find(|child| child.name() == name)
  }

  /// Iterates over all descendants of this node.
  ///
  /// Nodes are yielded in depth first pre-order traversal order. This node
  /// itself is not included.
  #[must_use]
  pub const fn descendants(&self) -> Descendants<'a> {
    Descendants::new(self.content, self.strings)
  }
}

impl<'a> Property<'a> {
  /// Returns the property's name.
  #[must_use]
  pub const fn name(&self) -> &'a [u8] {
    self.name
  }

  /// Returns the property's raw, uninterpreted value bytes.
  #[must_use]
  pub const fn value(&self) -> &'a [u8] {
    self.value
  }
}

impl<'a> Properties<'a> {
  /// Creates an iterator over the direct properties encoded in `content`.
  const fn new(content: &'a [u8], strings: Strings<'a>) -> Self {
    Self {
      reader: ValidatedReader::new(content),
      strings,
      finished: false,
    }
  }
}

impl<'a> Iterator for Properties<'a> {
  type Item = Property<'a>;

  fn next(&mut self) -> Option<Self::Item> {
    if self.finished {
      return None;
    }

    loop {
      match self.reader.read_token() {
        Token::Property => {
          return Some(self.reader.read_property(self.strings));
        }

        Token::Nop => {}

        Token::BeginNode | Token::EndNode => {
          self.finished = true;
          return None;
        }

        #[expect(
          clippy::unreachable,
          reason = "Structure::new validates that every node is closed before the final FDT_END token"
        )]
        Token::End => {
          unreachable!("validated node must end before the structure block terminator");
        }
      }
    }
  }
}

impl<'a> Children<'a> {
  /// Creates a child iterator beginning at the contents of a validated node.
  const fn new(content: &'a [u8], strings: Strings<'a>) -> Self {
    Self {
      reader: ValidatedReader::new(content),
      strings,
      depth: 0,
      finished: false,
    }
  }
}

impl<'a> Iterator for Children<'a> {
  type Item = Node<'a>;

  fn next(&mut self) -> Option<Self::Item> {
    if self.finished {
      return None;
    }

    loop {
      match self.reader.read_token() {
        Token::Property => {
          self.reader.skip_property();
        }

        Token::Nop => {}

        Token::BeginNode => {
          let name = self.reader.read_node_name();
          let content = self.reader.remaining_bytes();

          let is_direct_child = self.depth == 0;

          #[expect(
            clippy::arithmetic_side_effects,
            reason = "nesting depth is bounded by the validated finite structure block"
          )]
          {
            self.depth += 1;
          }

          if is_direct_child {
            return Some(Node::new(name, content, self.strings));
          }
        }

        Token::EndNode => {
          if self.depth == 0 {
            self.finished = true;
            return None;
          }

          #[expect(
            clippy::arithmetic_side_effects,
            reason = "the preceding zero check prevents underflow"
          )]
          {
            self.depth -= 1;
          }
        }

        #[expect(
          clippy::unreachable,
          reason = "Structure::new validates that every node is closed before the final FDT_END token"
        )]
        Token::End => {
          unreachable!("validated node must end before the structure block terminator");
        }
      }
    }
  }
}

impl<'a> Descendants<'a> {
  /// Creates a descendant iterator beginning at the contents of a validated node.
  const fn new(content: &'a [u8], strings: Strings<'a>) -> Self {
    Self {
      reader: ValidatedReader::new(content),
      strings,
      depth: 0,
      finished: false,
    }
  }
}

impl<'a> Iterator for Descendants<'a> {
  type Item = Node<'a>;

  fn next(&mut self) -> Option<Self::Item> {
    if self.finished {
      return None;
    }

    loop {
      match self.reader.read_token() {
        Token::Property => {
          self.reader.skip_property();
        }

        Token::Nop => {}

        Token::BeginNode => {
          let name = self.reader.read_node_name();
          let content = self.reader.remaining_bytes();
          #[expect(
            clippy::arithmetic_side_effects,
            reason = "nesting depth is bounded by the validated finite structure block"
          )]
          {
            self.depth += 1;
          }

          return Some(Node::new(name, content, self.strings));
        }

        Token::EndNode => {
          if self.depth == 0 {
            self.finished = true;
            return None;
          }
          #[expect(
            clippy::arithmetic_side_effects,
            reason = "the preceding zero check prevents underflow"
          )]
          {
            self.depth -= 1;
          }
        }

        #[expect(
          clippy::unreachable,
          reason = "Structure::new validates that every node is closed before the final FDT_END token"
        )]
        Token::End => {
          unreachable!("validated node must end before structure terminator");
        }
      }
    }
  }
}

impl<'a> Structure<'a> {
  /// Returns a read-only view of the validated structure's root node.
  pub(crate) fn root(&self, strings: Strings<'a>) -> Node<'a> {
    let mut reader = ValidatedReader::new(self.bytes);

    let token = reader.read_non_nop_token();

    debug_assert!(token == Token::BeginNode);

    let name = reader.read_node_name();

    debug_assert_eq!(name, []);

    Node::new(name, reader.remaining_bytes(), strings)
  }
}

#[cfg(test)]
mod test {
  use super::*;

  use crate::structure::test_utils::*;

  #[test]
  fn children_returns_only_direct_children() {
    let (structure_bytes, strings) = structure_with_nested_children();

    let structure = Structure::new(&structure_bytes, &strings).expect("structure should be valid");

    let root = structure.root(strings);

    let mut children = root.children();

    assert_eq!(
      children.next().map(|node| node.name()),
      Some(b"cpus".as_slice())
    );
    assert_eq!(
      children.next().map(|node| node.name()),
      Some(b"memory@0".as_slice())
    );
    assert_eq!(
      children.next().map(|node| node.name()),
      Some(b"soc".as_slice())
    );
    assert!(children.next().is_none());
  }

  #[test]
  fn nested_children_are_accessible_from_their_parent() {
    let (structure_bytes, strings) = structure_with_nested_children();

    let structure = Structure::new(&structure_bytes, &strings).expect("structure should be valid");

    let root = structure.root(strings);

    let soc = root.child(b"soc").expect("soc should exist");
    let uart = soc.child(b"uart").expect("uart should exist");

    assert_eq!(uart.name(), b"uart");
  }

  #[test]
  fn properties_returns_only_direct_properties() {
    let (structure_bytes, strings) = structure_with_nested_properties();

    let structure = Structure::new(&structure_bytes, &strings).expect("structure should be valid");

    let root = structure.root(strings);

    let property = root
      .property(b"root-property")
      .expect("root property should exist");

    assert_eq!(property.name(), b"root-property");
    assert_eq!(property.value(), &[1, 2, 3]);

    assert!(root.property(b"child-property").is_none());

    let child = root.child(b"child").expect("child should exist");

    let property = child
      .property(b"child-property")
      .expect("child property should exist");

    assert_eq!(property.value(), &[4, 5]);
  }

  #[test]
  fn traversing_returned_child_does_not_advance_parent_iterator() {
    let (structure_bytes, strings) = structure_with_nested_children();

    let structure = Structure::new(&structure_bytes, &strings).expect("structure should be valid");

    let root = structure.root(strings);

    let mut root_children = root.children();

    let cpus = root_children.next().expect("cpus should exist");
    assert_eq!(cpus.name(), b"cpus");

    // Traverse the returned Node using a completely independent reader.
    let cpu = cpus.children().next().expect("cpu should exist");
    assert_eq!(cpu.name(), b"cpu");

    // The original root iterator must still be capable of walking through the
    // remainder of `cpus` and finding the next root-level sibling.
    let memory = root_children.next().expect("memory should exist");
    assert_eq!(memory.name(), b"memory@0");

    assert!(root_children.next().is_some());
  }
}
