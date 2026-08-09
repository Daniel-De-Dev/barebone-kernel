//! Bootloader console output.
//!
//! This module provides the console backend used for text output.

#[cfg(target_arch = "riscv64")]
mod sbi;

#[cfg(target_arch = "riscv64")]
pub(crate) use sbi::SbiConsole as Console;
