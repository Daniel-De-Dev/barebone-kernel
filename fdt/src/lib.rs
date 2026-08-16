//! A `no_std` parser for flattened devicetree (FDT).
//!
//! Implementation based on Devicetree Specification v0.4.
//!
//! This crate provides the components used to parse and validate FDT, commonly
//! represented as a Device Tree Blob (DTB).
//!
//! Parsing is performed in stages corresponding to the physical layout of the
//! FDT. The [`header`] module handles the fixed-size header and validates the
//! offsets and sizes needed to safely access the variable-sized blocks that
//! follow it.

#![no_std]

// INFO: pub is placed temporary to let kernel access these directly
pub mod error;
pub mod header;
