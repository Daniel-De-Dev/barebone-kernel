//! RISC-V Supervisor Binary Interface support.
//!
//! This module implements the minimal subset of the SBI Debug Console
//! extension required by the bootloader, as specified by
//! [the RISC-V SBI documentation](https://github.com/riscv-non-isa/riscv-sbi-doc/blob/8a545effe9b50484ff897d9815d7d9015cdef203/src/ext-debug-console.adoc).

use core::arch::asm;

/// SBI Debug Console extension ID.
const DBCN_EID: usize = 0x4442_434E;

/// SBI Debug Console "write byte" function ID.
const DBCN_WRITE_BYTE_FID: usize = 2;

/// Errors returned when writing a byte through the SBI debug console.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WriteByteError {
  /// Failed to write the byte due to I/O errors.
  Failed,

  /// Write to the debug console is not allowed.
  Denied,
}

/// Writes one byte through the SBI debug console.
///
/// This is a blocking SBI call and returns only after the byte has been
/// written or the operation has failed.
///
/// # Errors
///
/// Returns [`WriteByteError::Denied`] when console output is not permitted,
/// [`WriteByteError::Failed`] when the write fails.
pub(crate) fn write_byte(byte: u8) -> Result<(), WriteByteError> {
  let error: isize;

  unsafe {
    asm!(
      "ecall",
      inlateout("a0") byte as usize => error,
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
    error => panic!("unexpected SBI debug console error: {error}"),
  }
}
