//! Kernel logging support.
//!
//! This module provides logging macros for formatting messages with a severity
//! level and source location before writing them to the kernel console.

use crate::console::Console;
use core::fmt::{self, Write};

/// Severity level of a log record.
#[derive(Clone, Copy)]
pub(super) enum Level {
  /// Detailed diagnostic information intended for debugging.
  Debug,

  /// Informational messages describing normal kernel operation.
  Info,

  /// Potential problems or unexpected conditions that do not prevent operation.
  Warn,

  /// Errors indicating that an operation or subsystem has failed.
  Error,
}

impl fmt::Display for Level {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    let level = match self {
      Self::Debug => "DEBUG",
      Self::Info => "INFO",
      Self::Warn => "WARN",
      Self::Error => "ERROR",
    };

    f.pad(level)
  }
}

/// Formats and writes a single log record to the kernel console.
///
/// Console output is best-effort. Write failures are ignored because the
/// logger currently has no independent fallback output path.
pub(super) fn write(level: Level, file: &str, line: u32, args: fmt::Arguments<'_>) {
  let mut console = Console;

  let _write_result = writeln!(console, "[{level:<5}] {file:>20}:{line:<3} | {args}");
}

/// Logs diagnostic information intended for debugging.
///
/// Debug messages are compiled only when debug assertions are enabled.
macro_rules! debug {
  ($($arg:tt)*) => {{
    #[cfg(debug_assertions)]
    {
      $crate::logging::write(
        $crate::logging::Level::Debug,
        file!(),
        line!(),
        format_args!($($arg)*),
      );
    }
  }};
}

/// Logs informational messages about normal kernel operation.
macro_rules! info {
  ($($arg:tt)*) => {
    $crate::logging::write(
      $crate::logging::Level::Info,
      file!(),
      line!(),
      format_args!($($arg)*),
    )
  };
}

/// Logs a potential problem or unexpected condition that does not prevent
/// continued operation.
macro_rules! warn {
  ($($arg:tt)*) => {
    $crate::logging::write(
      $crate::logging::Level::Warn,
      file!(),
      line!(),
      format_args!($($arg)*),
    )
  };
}

/// Logs an error indicating that an operation or subsystem has failed.
macro_rules! error {
  ($($arg:tt)*) => {
    $crate::logging::write(
      $crate::logging::Level::Error,
      file!(),
      line!(),
      format_args!($($arg)*),
    )
  };
}

pub(super) use {debug, error, info};
