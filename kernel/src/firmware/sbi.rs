//! RISC-V Supervisor Binary Interface support.
//!
//! This module implements the minimal subset of the SBI Debug Console
//! extension required by the kernel, as specified by
//! [the RISC-V SBI documentation](https://github.com/riscv-non-isa/riscv-sbi-doc/blob/8a545effe9b50484ff897d9815d7d9015cdef203/src/ext-debug-console.adoc).

use core::arch::asm;

/// SBI Debug Console extension ID.
const DBCN_EID: usize = 0x4442_434E;

/// SBI Debug Console "write byte" function ID.
const DBCN_WRITE_BYTE_FID: usize = 2;

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
  let error: isize;

  // SAFETY: `ecall` invokes the SBI implementation using the standard SBI
  // calling convention. All argument and return-value registers used by the
  // call are declared as assembly operands, and the call does not use the
  // current stack.
  unsafe {
    asm!(
      "ecall",
      inlateout("a0") usize::from(byte) => error,
      lateout("a1") _,
      in("a6") DBCN_WRITE_BYTE_FID,
      in("a7") DBCN_EID,
      options(nostack),
    );
  }

  match error {
    0 => Ok(()),
    -1 => Err(WriteByteError::Failed),
    -4 => Err(WriteByteError::Denied),
    error => Err(WriteByteError::Unexpected(error)),
  }
}
