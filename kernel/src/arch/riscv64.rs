//! RISC-V 64-bit architecture support for the kernel.
//!
//! This module provides the architecture-specific boundary between the
//! kernel and an RV64 execution environment.

mod boot;
mod interrupt;
mod paging;
mod time;
mod timer;
mod trap;

use core::arch::asm;

pub(crate) use interrupt::{
  InterruptGuard, disable as disable_interrupts, enable as enable_interrupts,
};
pub(crate) use paging::{BootstrapPaging, KernelPaging, PagingError};
pub(crate) use time::{init as init_time, ticks as time_ticks};
pub(crate) use timer::{TimerError, init as init_timer, ticks as timer_ticks};
pub(crate) use trap::init as init_trap;

/// Disables supervisor interrupt delivery and parks the current hart indefinitely.
///
/// This repeatedly executes the RISC-V `wfi` (Wait for Interrupt)
/// instruction. Because `wfi` may resume, it is executed in a loop so this
/// function never returns.
pub(crate) fn halt() -> ! {
  let _interrupts = disable_interrupts();

  loop {
    // SAFETY: `wfi` does not access Rust-managed memory or modify the stack.
    // Resuming from `wfi` is handled by the enclosing loop.
    unsafe {
      asm!("wfi", options(nomem, nostack));
    }
  }
}
