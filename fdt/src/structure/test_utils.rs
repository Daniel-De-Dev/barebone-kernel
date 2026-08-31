use super::*;

extern crate std;
use std::vec::Vec;

pub(super) const REG_OFFSET: u32 = 11;
pub(super) const ROOT_PROPERTY_OFFSET: u32 = 15;
pub(super) const CHILD_PROPERTY_OFFSET: u32 = 29;

const STRINGS: &[u8] = b"compatible\0\
reg\0\
root-property\0\
child-property\0";

pub(super) fn push_u32(bytes: &mut Vec<u8>, value: u32) {
  bytes.extend_from_slice(&value.to_be_bytes());
}

pub(super) fn push_token(bytes: &mut Vec<u8>, token: Token) -> usize {
  let offset = bytes.len();
  push_u32(bytes, token.value());
  offset
}

pub(super) fn pad_to_4(bytes: &mut Vec<u8>) {
  while bytes.len() % 4 != 0 {
    bytes.push(0);
  }
}

pub(super) fn push_begin_node(bytes: &mut Vec<u8>, name: &[u8]) -> usize {
  let offset = push_token(bytes, Token::BeginNode);

  bytes.extend_from_slice(name);
  bytes.push(0);
  pad_to_4(bytes);

  offset
}

pub(super) fn push_end_node(bytes: &mut Vec<u8>) -> usize {
  push_token(bytes, Token::EndNode)
}

pub(super) fn push_property(bytes: &mut Vec<u8>, name_offset: u32, value: &[u8]) -> usize {
  let offset = push_token(bytes, Token::Property);

  push_u32(bytes, value.len() as u32);
  push_u32(bytes, name_offset);
  bytes.extend_from_slice(value);
  pad_to_4(bytes);

  offset
}

pub(super) fn push_nop(bytes: &mut Vec<u8>) -> usize {
  push_token(bytes, Token::Nop)
}

pub(super) fn push_end(bytes: &mut Vec<u8>) -> usize {
  push_token(bytes, Token::End)
}

pub(super) fn strings() -> Strings<'static> {
  Strings::new(STRINGS)
}

pub(super) fn push_required_root_nodes(bytes: &mut Vec<u8>) {
  push_begin_node(bytes, b"cpus");
  push_end_node(bytes);

  push_begin_node(bytes, b"memory@0");
  push_property(bytes, REG_OFFSET, b"0");
  push_end_node(bytes);
}

pub(super) fn structure_with_nested_children() -> (Vec<u8>, Strings<'static>) {
  let mut bytes = Vec::new();

  push_nop(&mut bytes);
  push_nop(&mut bytes);
  push_begin_node(&mut bytes, b"");

  push_nop(&mut bytes);

  push_begin_node(&mut bytes, b"cpus");

  push_begin_node(&mut bytes, b"cpu");
  push_end_node(&mut bytes);

  push_end_node(&mut bytes);

  push_begin_node(&mut bytes, b"memory@0");
  push_property(&mut bytes, REG_OFFSET, b"0");
  push_end_node(&mut bytes);

  push_begin_node(&mut bytes, b"soc");

  push_begin_node(&mut bytes, b"uart");
  push_end_node(&mut bytes);

  push_end_node(&mut bytes);

  push_end_node(&mut bytes);
  push_end(&mut bytes);

  (bytes, strings())
}

pub(super) fn structure_with_nested_properties() -> (Vec<u8>, Strings<'static>) {
  let mut bytes = Vec::new();

  push_begin_node(&mut bytes, b"");

  push_nop(&mut bytes);
  push_nop(&mut bytes);
  push_property(&mut bytes, ROOT_PROPERTY_OFFSET, &[1, 2, 3]);

  push_nop(&mut bytes);
  push_nop(&mut bytes);
  push_begin_node(&mut bytes, b"child");

  push_property(&mut bytes, CHILD_PROPERTY_OFFSET, &[4, 5]);

  push_end_node(&mut bytes);

  push_required_root_nodes(&mut bytes);

  push_end_node(&mut bytes);
  push_end(&mut bytes);

  (bytes, strings())
}
