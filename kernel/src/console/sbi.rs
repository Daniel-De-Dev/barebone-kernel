//! SBI-backed console output.

use core::fmt;

use crate::firmware::sbi;

/// Console writer backed by the SBI debug console.
pub(crate) struct SbiConsole;

impl SbiConsole {
  /// Writes one uninterpreted byte through the firmware console.
  ///
  /// # Errors
  ///
  /// Returns an error if firmware rejects the byte.
  pub(crate) fn write_byte(byte: u8) -> fmt::Result {
    sbi::write_byte(byte).map_err(|_| fmt::Error)
  }
}

// TODO: Add extra robustness logic for later to query the SBI if it has the
// debug console extension implemented and not explicitly assume it.
impl fmt::Write for SbiConsole {
  fn write_str(&mut self, string: &str) -> fmt::Result {
    for byte in string.bytes() {
      Self::write_byte(byte)?;
    }

    Ok(())
  }
}
