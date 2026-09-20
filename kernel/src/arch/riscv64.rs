//! RISC-V 64-bit architecture support for the kernel.
//!
//! This module provides the architecture-specific boundary between the
//! kernel and an RV64 execution environment.

mod boot;
mod trap;
pub(crate) use trap::init as init_trap;

use core::arch::asm;

/// Parks the current hart indefinitely.
///
/// This repeatedly executes the RISC-V `wfi` (Wait for Interrupt)
/// instruction. Because `wfi` may resume, it is executed in a loop so this
/// function never returns.
pub(crate) fn halt() -> ! {
  loop {
    // SAFETY: `wfi` does not access Rust-managed memory or modify the stack.
    // Resuming from `wfi` is handled by the enclosing loop.
    unsafe {
      asm!("wfi", options(nomem, nostack));
    }
  }
}
