//! A `no_std` parser and validator for flattened devicetrees (FDT).
//!
//! [`Fdt`] provides a validated, borrowed view of a Device Tree Blob (DTB).
//! Construction validates the binary structure required to safely interpret
//! the blob.
//!
//! Validation is structural. It does not establish that the represented
//! devicetree satisfies device-specific or binding-specific semantic
//! requirements.
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
pub use structure::{NodeNameError, StructureError};

#[cfg(target_pointer_width = "16")]
compile_error!("the fdt crate requires a target pointer width of at least 32 bits");
