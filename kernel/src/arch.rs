//! Architecture-specific kernel support.
//!
//! This module provides a common interface to functionality whose
//! implementation depends on the target architecture.

#[cfg(target_arch = "riscv64")]
mod riscv64;

#[cfg(target_arch = "riscv64")]
pub(super) use riscv64::{
  BootstrapPaging, InterruptGuard, KernelPaging, PagingError, TimerError, UserContext,
  disable_interrupts, enable_interrupts, enter_user, halt, idle, init_time, init_timer, init_trap,
  time_ticks, timer_ticks, user_timer_ticks,
};
