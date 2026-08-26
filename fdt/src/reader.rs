//! Bounded sequential reading from a byte slice.
//!
//! [`Reader`] maintains a cursor into an immutable byte slice and provides
//! operations for consuming data while advancing that cursor.

/// Errors produced while reading from a bounded byte slice.
#[derive(Debug, PartialEq)]
pub enum ReadError {
  /// The requested number of bytes was not available.
  Truncated {
    /// Offset at which the read was attempted, relative to the start of the slice.
    offset: usize,

    /// Number of bytes requested.
    requested: usize,

    /// Number of bytes available from `offset`.
    remaining: usize,
  },

  /// No NUL terminator was found in the remaining bytes.
  MissingNulTerminator {
    /// Offset at which the byte sequence begins, relative to the start of the slice.
    offset: usize,
  },
}

/// A sequential reader over a bounded byte slice.
///
/// The cursor always lies within the underlying slice. Successful reads advance
/// the cursor by the number of bytes consumed; failed reads leave it unchanged.
pub(super) struct Reader<'a> {
  /// Underlying byte slice being read.
  bytes: &'a [u8],

  /// Current byte offset from the start of `bytes`.
  ///
  /// Always less than or equal to `bytes.len()`.
  offset: usize,
}

impl<'a> Reader<'a> {
  /// Creates a reader positioned at the beginning of `bytes`.
  pub(super) const fn new(bytes: &'a [u8]) -> Self {
    Self { bytes, offset: 0 }
  }

  /// Returns the current byte offset from the start of the slice.
  pub(super) const fn position(&self) -> usize {
    self.offset
  }

  /// Returns the number of unread bytes.
  pub(super) const fn remaining(&self) -> usize {
    self.bytes.len() - self.offset
  }

  /// Reads a big-endian `u32`.
  ///
  /// # Errors
  ///
  /// Returns [`ReadError::Truncated`] if fewer than four bytes remain.
  pub(super) fn read_u32(&mut self) -> Result<u32, ReadError> {
    let bytes = self.read_bytes(4)?;

    Ok(u32::from_be_bytes(
      bytes.try_into().expect("slice length is known to be 4"),
    ))
  }

  /// Reads `length` bytes.
  ///
  /// # Errors
  ///
  /// Returns [`ReadError::Truncated`] if `length` exceeds the number of
  /// remaining bytes.
  pub(super) fn read_bytes(&mut self, length: usize) -> Result<&'a [u8], ReadError> {
    let remaining = self.remaining();

    if length > remaining {
      return Err(ReadError::Truncated {
        offset: self.offset,
        requested: length,
        remaining,
      });
    }

    let start = self.offset;
    let end = start + length;

    self.offset = end;

    Ok(&self.bytes[start..end])
  }

  /// Advances the cursor to the next 4-byte boundary.
  ///
  /// Returns the bytes consumed to reach the boundary. Their contents are not
  /// interpreted or validated.
  ///
  /// # Errors
  ///
  /// Returns [`ReadError::Truncated`] if the required padding is not available.
  pub(super) fn align_to_4(&mut self) -> Result<&'a [u8], ReadError> {
    let padding = (4 - (self.offset % 4)) % 4;

    self.read_bytes(padding)
  }

  /// Reads bytes up to and excluding the next NUL terminator.
  ///
  /// The terminator is consumed but is not included in the returned slice.
  ///
  /// # Errors
  ///
  /// Returns [`ReadError::MissingNulTerminator`] if no NUL terminator exists in
  /// the remaining bytes.
  pub(super) fn read_nul_terminated(&mut self) -> Result<&'a [u8], ReadError> {
    let start = self.offset;
    let remaining = &self.bytes[start..];

    let length = remaining
      .iter()
      .position(|&byte| byte == 0)
      .ok_or(ReadError::MissingNulTerminator { offset: start })?;

    let bytes = self
      .read_bytes(length + 1)
      .expect("NUL terminator is known to lie within the remaining bytes");

    Ok(&bytes[..length])
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn new_reader_starts_at_beginning_of_slice() {
    let data = [0; 5];
    let reader = Reader::new(&data);

    assert_eq!(reader.position(), 0);
    assert_eq!(reader.remaining(), 5);
  }

  #[test]
  fn u32_is_read_and_cursor_advances() {
    let data = [0x12, 0x34, 0x56, 0x78, 0xff];
    let mut reader = Reader::new(&data);

    let result = reader.read_u32().expect("failed to read u32");

    assert_eq!(result, 0x1234_5678);
    assert_eq!(reader.position(), 4);
    assert_eq!(reader.remaining(), 1);
  }

  #[test]
  fn truncated_u32_is_rejected_without_advancing() {
    let data = [0; 3];
    let mut reader = Reader::new(&data);

    assert_eq!(
      reader.read_u32(),
      Err(ReadError::Truncated {
        offset: 0,
        requested: 4,
        remaining: 3,
      })
    );

    assert_eq!(reader.position(), 0);
    assert_eq!(reader.remaining(), 3);
  }

  #[test]
  fn oversized_read_is_rejected_without_advancing() {
    let data = [0; 2];
    let mut reader = Reader::new(&data);

    reader.read_bytes(1).expect("failed to read initial byte");

    assert_eq!(
      reader.read_bytes(usize::MAX),
      Err(ReadError::Truncated {
        offset: 1,
        requested: usize::MAX,
        remaining: 1,
      })
    );

    assert_eq!(reader.position(), 1);
  }

  #[test]
  fn bytes_are_read_and_cursor_advances() {
    let data = [0x01, 0x02, 0x03, 0x04];
    let mut reader = Reader::new(&data);

    let bytes = reader.read_bytes(2).expect("failed to read bytes");

    assert_eq!(bytes, &[0x01, 0x02]);
    assert_eq!(reader.position(), 2);
    assert_eq!(reader.remaining(), 2);
  }

  #[test]
  fn truncated_byte_read_is_rejected_without_advancing() {
    let data = [0; 2];
    let mut reader = Reader::new(&data);

    reader.read_bytes(1).expect("failed to read initial byte");

    assert_eq!(
      reader.read_bytes(2),
      Err(ReadError::Truncated {
        offset: 1,
        requested: 2,
        remaining: 1,
      })
    );

    assert_eq!(reader.position(), 1);
    assert_eq!(reader.remaining(), 1);
  }

  #[test]
  fn zero_length_read_does_not_advance() {
    let data = [0; 2];
    let mut reader = Reader::new(&data);

    assert_eq!(reader.read_bytes(0), Ok(&[][..]));
    assert_eq!(reader.position(), 0);
    assert_eq!(reader.remaining(), 2);
  }

  #[test]
  fn alignment_consumes_bytes_to_next_4_byte_boundary() {
    let data = [0xff, 0x01, 0x02, 0x03, 0xff];
    let mut reader = Reader::new(&data);

    reader.read_bytes(1).expect("failed to read initial byte");

    let padding = reader.align_to_4().expect("failed to align reader");

    assert_eq!(padding, &[0x01, 0x02, 0x03]);
    assert_eq!(reader.position(), 4);
    assert_eq!(reader.remaining(), 1);
  }

  #[test]
  fn alignment_does_not_advance_when_already_aligned() {
    let data = [0; 8];
    let mut reader = Reader::new(&data);

    reader.read_bytes(4).expect("failed to read initial bytes");

    reader.align_to_4().expect("failed to align reader");

    assert_eq!(reader.position(), 4);
    assert_eq!(reader.remaining(), 4);
  }

  #[test]
  fn truncated_alignment_is_rejected_without_advancing() {
    let data = [0; 2];
    let mut reader = Reader::new(&data);

    reader.read_bytes(1).expect("failed to read initial byte");

    assert_eq!(
      reader.align_to_4(),
      Err(ReadError::Truncated {
        offset: 1,
        requested: 3,
        remaining: 1,
      })
    );

    assert_eq!(reader.position(), 1);
  }

  #[test]
  fn nul_terminated_bytes_are_read_and_cursor_advances() {
    let data = [b'h', b'e', b'l', b'l', b'o', 0, 0xff];
    let mut reader = Reader::new(&data);

    let result = reader
      .read_nul_terminated()
      .expect("failed to read nul-terminated slice");

    assert_eq!(result, b"hello");
    assert_eq!(reader.position(), 6);
    assert_eq!(reader.remaining(), 1);
  }

  #[test]
  fn empty_nul_terminated_slice_is_accepted() {
    let data = [0, 0xff];
    let mut reader = Reader::new(&data);

    assert_eq!(reader.read_nul_terminated(), Ok(&[][..]));
    assert_eq!(reader.position(), 1);
    assert_eq!(reader.remaining(), 1);
  }

  #[test]
  fn missing_nul_terminator_is_rejected_without_advancing() {
    let data = [b'h', b'e', b'l', b'l', b'o'];
    let mut reader = Reader::new(&data);

    assert_eq!(
      reader.read_nul_terminated(),
      Err(ReadError::MissingNulTerminator { offset: 0 })
    );

    assert_eq!(reader.position(), 0);
  }

  #[test]
  fn missing_nul_terminator_reports_current_offset() {
    let data = [0xff, b'a', b'b'];
    let mut reader = Reader::new(&data);

    reader.read_bytes(1).expect("failed to read initial byte");

    assert_eq!(
      reader.read_nul_terminated(),
      Err(ReadError::MissingNulTerminator { offset: 1 })
    );

    assert_eq!(reader.position(), 1);
  }

  #[test]
  fn nul_terminator_at_end_consumes_entire_slice() {
    let data = [b'a', b'b', b'c', 0];
    let mut reader = Reader::new(&data);

    assert_eq!(reader.read_nul_terminated(), Ok(b"abc".as_slice()));
    assert_eq!(reader.position(), data.len());
    assert_eq!(reader.remaining(), 0);
  }
}
