mod sbi;

use self::sbi::SbiConsole;
use core::fmt::Write;
use log::{Log, Metadata, Record, SetLoggerError};

static LOGGER: BootLogger = BootLogger;

struct BootLogger;

impl Log for BootLogger {
  fn enabled(&self, metadata: &Metadata) -> bool {
    metadata.level() <= log::max_level()
  }

  fn log(&self, record: &Record) {
    if !self.enabled(record.metadata()) {
      return;
    }

    let mut console = SbiConsole;

    let file = record.file().unwrap_or("");
    let line = record.line().unwrap_or(0);

    let _ = writeln!(
      console,
      "[{level:<5}] {file:>20}:{line:<3} | {args}",
      level = record.level(),
      file = file,
      line = line,
      args = record.args(),
    );
  }

  fn flush(&self) {}
}

pub fn init() -> Result<(), SetLoggerError> {
  log::set_logger(&LOGGER)?;
  log::set_max_level(log::STATIC_MAX_LEVEL);

  Ok(())
}
