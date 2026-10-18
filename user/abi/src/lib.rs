//! Scalar syscall numbers and results shared by the kernel and user programs.
//!
//! RV64 callers place the number in a7 and the argument in a0, then execute
//! ecall. Returning calls replace a0 and preserve every other integer register.
//! Errors use a negative errno encoded in a machine-sized unsigned integer.

#![no_std]

/// Writes the byte supplied in a0 to the console, rejecting values above 255.
pub const CONSOLE_WRITE_BYTE: usize = 1;

/// Ends execution with the status supplied in a0 and never returns.
pub const EXIT: usize = 2;

/// Returns the wrapping count of timer interrupts delivered from user mode.
pub const TIMER_TICKS: usize = 3;

/// A console operation failed, encoded as -5.
pub const ERROR_IO: usize = 5usize.wrapping_neg();

/// A scalar argument was invalid, encoded as -22.
pub const ERROR_INVALID_ARGUMENT: usize = 22usize.wrapping_neg();

/// The requested syscall is unsupported, encoded as -38.
pub const ERROR_NOT_SUPPORTED: usize = 38usize.wrapping_neg();
