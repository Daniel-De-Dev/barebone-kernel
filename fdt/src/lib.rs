//! A `no_std` parser and validator for flattened devicetrees (FDT).
//!
//! [`Fdt`] provides a validated, borrowed view of a Device Tree Blob (DTB).
//! Construction performs the validation required before the blob is exposed
//! through the public API.
//!
//! The guarantees provided by a constructed [`Fdt`] do not imply conformance
//! to device-, bus-, or binding-specific semantic requirements.
//!
//! Implementation based on [Devicetree Specification v0.4](https://www.devicetree.org/).

// TODO: make the test compliant with Clippy (cannot be asked)
// `cargo clippy --all-targets`
// also look more into actual u32 bit support?

#![no_std]

mod error;
mod fdt;
mod header;
mod helpers;
mod reader;
mod reservation;
mod strings;
mod structure;

pub use error::Error;
pub use fdt::{BlobError, Fdt};
pub use header::{BlockKind, HeaderError};
pub use reader::ReadError;
pub use strings::PropertyNameError;
pub use structure::{
  Children, Descendants, Node, NodeNameError, Properties, Property, StructureError,
};

#[cfg(target_pointer_width = "16")]
compile_error!("the fdt crate requires a target pointer width of at least 32 bits");
