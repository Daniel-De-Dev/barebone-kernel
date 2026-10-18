//! RISC-V Supervisor Binary Interface support.
//!
//! Debug Console output and absolute timer deadlines through SBI calls.

use core::arch::asm;

/// SBI Debug Console extension ID.
const DBCN_EID: usize = 0x4442_434E;

/// SBI Debug Console "write byte" function ID.
const DBCN_WRITE_BYTE_FID: usize = 2;

/// SBI Timer extension ID.
const TIME_EID: usize = 0x5449_4d45;

/// SBI Timer "set timer" function ID.
const SET_TIMER_FID: usize = 0;

/// Failures while programming an SBI timer deadline.
#[derive(Debug)]
pub(crate) enum SetTimerError {
  /// The firmware does not support the Timer extension.
  NotSupported,

  /// Timer programming is not permitted.
  Denied,

  /// The firmware returned another error code.
  Unexpected(isize),
}

/// Errors returned when writing a byte through the SBI debug console.
#[derive(Debug)]
pub(crate) enum WriteByteError {
  /// Failed to write the byte due to I/O errors.
  Failed,

  /// Write to the debug console is not allowed.
  Denied,

  /// The SBI implementation returned an error not expected for this operation.
  Unexpected(isize),
}

/// Writes one byte through the SBI debug console.
///
/// This is a blocking SBI call and returns only after the byte has been
/// written or the operation has failed.
///
/// # Errors
///
/// Returns:
///
/// - [`WriteByteError::Denied`] if console output is not permitted.
/// - [`WriteByteError::Failed`] if the write fails.
/// - [`WriteByteError::Unexpected`] if the SBI implementation returns an
///   error not defined for this operation.
pub(crate) fn write_byte(byte: u8) -> Result<(), WriteByteError> {
  match call(DBCN_EID, DBCN_WRITE_BYTE_FID, u64::from(byte)) {
    0 => Ok(()),
    -1 => Err(WriteByteError::Failed),
    -4 => Err(WriteByteError::Denied),
    error => Err(WriteByteError::Unexpected(error)),
  }
}

/// Programs an absolute deadline in time-counter ticks.
///
/// A future deadline clears the pending supervisor timer interrupt. This call
/// does not enable interrupt delivery.
///
/// # Errors
///
/// Returns an error if the extension is unavailable or firmware rejects the call.
pub(crate) fn set_timer(deadline: u64) -> Result<(), SetTimerError> {
  match call(TIME_EID, SET_TIMER_FID, deadline) {
    0 => Ok(()),
    -2 => Err(SetTimerError::NotSupported),
    -4 => Err(SetTimerError::Denied),
    error => Err(SetTimerError::Unexpected(error)),
  }
}

/// Calls an SBI function with one argument and returns its error code.
fn call(extension: usize, function: usize, argument: u64) -> isize {
  let error: isize;

  // SAFETY: `ecall` invokes the SBI implementation using the standard SBI
  // calling convention. All argument and return-value registers used by the
  // call are declared as assembly operands, and the call does not use the
  // current stack.
  unsafe {
    asm!(
      "ecall",
      inlateout("a0") argument => error,
      lateout("a1") _,
      in("a6") function,
      in("a7") extension,
      options(nostack),
    );
  }

  error
}
