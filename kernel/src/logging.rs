//! Kernel logging through the console.
//!
//! Every record has a severity and a subsystem target. Info, warning, and error
//! records are always compiled. Debug records require log-debug; trace records
//! require log-trace, which also enables log-debug through Cargo.
//!
//! Debug and trace records additionally require the matching target feature:
//! log-boot, log-frames, or log-paging. These controls are independent of the
//! build profile and debug assertions.
//!
//! Disabled macro calls are removed with cfg attributes, including their
//! argument expressions. Keep kernel operations outside logging arguments.
//! Diagnostic work performed before a call needs its own matching cfg guard.
//!
//! After the one-time architecture probe, records include a `time` field only
//! if the current hart can read the time CSR.

use crate::{arch, console::Console};
use core::fmt::{self, Write};

/// Severity level of a log record.
#[derive(Clone, Copy)]
pub(super) enum Level {
  /// Individual operations, such as allocating one physical frame.
  #[cfg(feature = "log-trace")]
  Trace,

  /// Diagnostic summaries and decisions intended for debugging.
  #[cfg(feature = "log-debug")]
  Debug,

  /// Informational messages describing normal kernel operation.
  Info,

  /// Potential problems that do not prevent continued operation.
  Warn,

  /// Errors indicating that an operation or subsystem has failed.
  Error,
}

impl fmt::Display for Level {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    let level = match *self {
      #[cfg(feature = "log-trace")]
      Self::Trace => "TRACE",
      #[cfg(feature = "log-debug")]
      Self::Debug => "DEBUG",
      Self::Info => "INFO",
      Self::Warn => "WARN",
      Self::Error => "ERROR",
    };

    f.pad(level)
  }
}

/// Subsystem responsible for a log record.
#[derive(Clone, Copy)]
pub(super) enum Target {
  /// Kernel startup, firmware information, and early trap handling.
  Boot,

  /// Physical frame discovery and allocation.
  Frames,

  /// Address-space construction and physical-frame mappings.
  Paging,
}

impl fmt::Display for Target {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    let target = match *self {
      Self::Boot => "boot",
      Self::Frames => "frames",
      Self::Paging => "paging",
    };

    f.pad(target)
  }
}

/// Raw timebase ticks, formatted without allocation.
struct Timestamp(Option<u64>);

impl fmt::Display for Timestamp {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    self
      .0
      .map_or(Ok(()), |ticks| write!(formatter, "[{ticks:>12}]"))
  }
}

/// Formats and writes one record directly to the kernel console.
///
/// Console output is best-effort. Write failures are ignored because there is
/// no independent fallback output path. This writer must not allocate memory
/// or use the paging frame window since those operations may themselves log.
///
/// Output currently relies on the kernel's single-hart execution.
pub(super) fn write(level: Level, target: Target, file: &str, line: u32, args: fmt::Arguments<'_>) {
  let time = Timestamp(arch::time_ticks());
  let mut console = Console;

  let file = file.strip_prefix("kernel/src/").unwrap_or(file);

  let _write_result = writeln!(
    console,
    "[{level:<5}][{target:<6}]{time} {file:>32}:{line:<4} | {args}"
  );
}

/// Emits an enabled record, preserving the original caller's source location.
macro_rules! record {
  ($level:ident, $target:ident, $($arg:tt)+) => {
    $crate::logging::write(
      $crate::logging::Level::$level,
      $crate::logging::Target::$target,
      file!(),
      line!(),
      format_args!($($arg)+),
    )
  };
}

/// Selects the compile-time subsystem guard for a verbose record.
macro_rules! verbose {
  (Boot, $level:ident, $($arg:tt)+) => {
    $crate::logging::verbose!("log-boot", $level, Boot, $($arg)+)
  };
  (Frames, $level:ident, $($arg:tt)+) => {
    $crate::logging::verbose!("log-frames", $level, Frames, $($arg)+)
  };
  (Paging, $level:ident, $($arg:tt)+) => {
    $crate::logging::verbose!("log-paging", $level, Paging, $($arg)+)
  };
  ($feature:literal, $level:ident, $target:ident, $($arg:tt)+) => {{
    #[cfg(feature = $feature)]
    {
      $crate::logging::record!($level, $target, $($arg)+);
    }
  }};
}

/// Logs an individual operation when log-trace and its target are enabled.
macro_rules! trace {
  ($target:ident, $($arg:tt)+) => {{
    #[cfg(feature = "log-trace")]
    {
      $crate::logging::verbose!($target, Trace, $($arg)+);
    }
  }};
}

/// Logs a diagnostic summary when log-debug and its target are enabled.
macro_rules! debug {
  ($target:ident, $($arg:tt)+) => {{
    #[cfg(feature = "log-debug")]
    {
      $crate::logging::verbose!($target, Debug, $($arg)+);
    }
  }};
}

/// Logs normal kernel operation regardless of the verbose logging features.
macro_rules! info {
  ($target:ident, $($arg:tt)+) => {
    $crate::logging::record!(Info, $target, $($arg)+)
  };
}

/// Logs a recoverable problem regardless of the verbose logging features.
macro_rules! warning {
  ($target:ident, $($arg:tt)+) => {
    $crate::logging::record!(Warn, $target, $($arg)+)
  };
}

/// Logs an operation failure regardless of the verbose logging features.
macro_rules! error {
  ($target:ident, $($arg:tt)+) => {
    $crate::logging::record!(Error, $target, $($arg)+)
  };
}

pub(super) use {debug, error, info, record, trace, verbose, warning as warn};
