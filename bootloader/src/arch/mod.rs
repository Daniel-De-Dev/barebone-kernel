//! Architecture-specific bootloader support.
//!
//! This module provides a common interface to functionality whose
//! implementation depends on the target architecture.

#[cfg(target_arch = "riscv64")]
mod riscv64;

#[cfg(target_arch = "riscv64")]
pub use riscv64::halt;
