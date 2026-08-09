//! RISC-V 64-bit architecture support for the bootloader.
//!
//! This module provides the architecture-specific boundary between the
//! bootloader's Rust code and an RV64 execution environment.
//!
//! It is responsible for the earliest CPU setup required before entering
//! Rust and for CPU-specific primitives used by the rest of the
//! bootloader.
//!
//! # Entry
//!
//! The `_start` entry point prepares the execution environment expected by
//! Rust code:
//!
//! 1. Initialize the global pointer (`gp`).
//! 2. Initialize the stack pointer (`sp`).
//! 3. Clear the `.bss` section.
//! 4. Transfer control to [`crate::main`].
//!
//! The firmware-provided `a0` and `a1` registers are left untouched so that
//! they are passed to `main` as the hart ID and device-tree address,
//! respectively.
//!
//! The linker script provides the symbols used during initialization,
//! including `__global_pointer$`, `_stack_end`, `_bss_start`, and `_bss_end`.
//!
//! The linker script keeps `_bss_start` and `_bss_end` 8-byte aligned because
//! startup clears `.bss` using 8-byte stores.

use core::arch::{asm, global_asm};

// Bootloader entry point.
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

    /* Initilize stack */
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
pub fn halt() -> ! {
  loop {
    unsafe {
      asm!("wfi", options(nomem, nostack));
    }
  }
}
