//! Bootloader logging support.
//!
//! This module provides lightweight logging macros that format messages and
//! write them to the bootloader console.

use crate::console::Console;
use core::fmt::{self, Write};

/// Logging severity.
pub(crate) enum Level {
  /// Detailed diagnostic information useful during debugging.
  Debug,

  /// General information about normal operation.
  Info,
  // Warn,
  // Error,
}

impl fmt::Display for Level {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    let level = match self {
      Self::Debug => "DEBUG",
      Self::Info => "INFO",
      // Self::Warn => f.write_str("WARN"),
      // Self::Error => f.write_str("ERROR"),
    };

    f.pad(level)
  }
}

/// Writes one formatted log record to the bootloader console.
pub(crate) fn write(level: Level, file: &str, line: u32, args: fmt::Arguments<'_>) {
  let mut console = Console;

  if writeln!(console, "[{level:<5}] {file:>20}:{line:<3} | {args}").is_err() {
    todo!("implement emergency output for console write failures");
  }
}

/// Logs diagnostic information useful while debugging.
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

/// Logs information about normal bootloader operation.
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

// macro_rules! warn {
//   ($($arg:tt)*) => {
//     $crate::logging::write(
//       $crate::logging::Level::Warn,
//       file!(),
//       line!(),
//       format_args!($($arg)*),
//     )
//   };
// }

// macro_rules! error {
//   ($($arg:tt)*) => {
//     $crate::logging::write(
//       $crate::logging::Level::Error,
//       file!(),
//       line!(),
//       format_args!($($arg)*),
//     )
//   };
// }

pub(crate) use {debug, info};
