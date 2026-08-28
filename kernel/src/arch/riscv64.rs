//! RISC-V 64-bit architecture support for the kernel.
//!
//! This module provides the architecture-specific boundary between the
//! kernel and an RV64 execution environment.
//!
//! # Kernel entry
//!
//! The `_start` entry point establishes the minimum execution environment
//! required before entering Rust:
//!
//! 1. Initialize the global pointer (`gp`).
//! 2. Initialize the stack pointer (`sp`).
//! 3. Clear the `.bss` section.
//! 4. Transfer control to [`crate::main`].
//!
//! The firmware-provided `a0` and `a1` registers are deliberately preserved
//! so they are passed to `main` as the hart ID and device-tree address,
//! respectively.
//!
//! The linker script provides the symbols used during initialization,
//! including `__global_pointer$`, `_stack_end`, `_bss_start`, and `_bss_end`.
//!
//! `_bss_start` and `_bss_end` are 8-byte aligned because startup clears
//! `.bss` using 8-byte stores.

mod trap;
pub(crate) use trap::init as init_trap;

use core::arch::{asm, global_asm};

// Kernel entry point.
//
// Establish the minimum execution environment required by Rust before
// transferring control to `main`.
//
// `a0` and `a1` are deliberately preserved so they remain the first two
// arguments passed to `main`.
global_asm!(
  r#"
  .section .text.init
  .global _start

_start:
  .option push
  .option norelax

  /* Initialize gp */
  la gp, __global_pointer$

  .option pop

  /* Initialize stack */
  la sp, _stack_end

  /* Clear .bss */
  la t0, _bss_start
  la t1, _bss_end

.bss_loop:
  bgeu t0, t1, .bss_done

  sd zero, 0(t0)
  addi t0, t0, 8

  j .bss_loop

.bss_done:
  tail main
  "#
);

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
