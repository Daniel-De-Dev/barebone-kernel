//! Firmware interfaces used by the bootloader.
//!
//! This module provides access to services exposed by the firmware environment
//! in which the bootloader executes.

#[cfg(target_arch = "riscv64")]
pub(crate) mod sbi;
