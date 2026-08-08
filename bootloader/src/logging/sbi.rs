use core::fmt;

pub(super) struct SbiConsole;

impl fmt::Write for SbiConsole {
  fn write_str(&mut self, string: &str) -> fmt::Result {
    for byte in string.bytes() {
      let ret = sbi::debug_console::write_byte(byte);

      if ret.is_err() {
        return Err(fmt::Error);
      }
    }

    Ok(())
  }
}
