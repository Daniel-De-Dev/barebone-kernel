//! Error types produced while constructing and validating an FDT.
//!
//! [`Error`] is the top-level error returned by the parser. More specific error
//! types are preserved as variants so callers can inspect the stage at which
//! validation failed.

use crate::{fdt::BlobError, header::HeaderError, reader::ReadError, structure::StructureError};

/// An error encountered while constructing or validating an FDT.
///
/// Each variant preserves the error produced by the component that detected the
/// failure.
#[derive(Debug, PartialEq, Eq)]
pub enum Error {
  /// An error occurred while establishing the DTB byte range.
  Blob(BlobError),

  /// An error occurred while decoding or validating the FDT header.
  Header(HeaderError),

  /// An error occurred while validating the FDT structure block.
  Structure(StructureError),

  /// An error occurred while validating the FDT memory reservation block.
  Reservation(ReadError),
}

/// Converts a blob-level error into the top-level FDT error type.
impl From<BlobError> for Error {
  fn from(error: BlobError) -> Self {
    Self::Blob(error)
  }
}

/// Converts a header error into the top-level FDT error type.
impl From<HeaderError> for Error {
  fn from(error: HeaderError) -> Self {
    Self::Header(error)
  }
}

/// Converts a structure error into the top-level FDT error type.
impl From<StructureError> for Error {
  fn from(error: StructureError) -> Self {
    Self::Structure(error)
  }
}
