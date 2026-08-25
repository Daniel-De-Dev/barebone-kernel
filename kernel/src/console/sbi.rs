//! SBI-backed console output.

use crate::firmware::sbi;
use core::fmt;

/// Console writer backed by the SBI debug console.
pub(crate) struct SbiConsole;

// TODO: Add extra robustness logic for later to query the SBI if it has the
// debug console extension implemented and not explicitly assume it.
impl fmt::Write for SbiConsole {
  fn write_str(&mut self, string: &str) -> fmt::Result {
    for byte in string.bytes() {
      sbi::write_byte(byte).map_err(|_| fmt::Error)?;
    }

    Ok(())
  }
}
