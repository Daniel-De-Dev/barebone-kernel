//! Kernel console output.
//!
//! This module provides the console backend used for text output.

#[cfg(target_arch = "riscv64")]
mod sbi;

// TODO: In future add extra logic for how a console is selected.
// (So kernel picks it dynamically in some specified order)
#[cfg(target_arch = "riscv64")]
pub(super) use sbi::SbiConsole as Console;
