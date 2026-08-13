//! Firmware interfaces used by the kernel.
//!
//! This module provides access to services exposed by the firmware environment
//! in which the kernel executes.

#[cfg(target_arch = "riscv64")]
pub(crate) mod sbi;
