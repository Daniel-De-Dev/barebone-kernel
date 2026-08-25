//! Firmware interfaces used by the kernel.
//!
//! This module provides access to services exposed by the firmware environment
//! in which the kernel executes.
// TODO: In distant future add "targets" the kernel could be compiled for

#[cfg(target_arch = "riscv64")]
pub(super) mod sbi;
