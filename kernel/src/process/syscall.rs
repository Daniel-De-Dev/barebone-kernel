//! Scalar-only user syscall dispatch.

use crate::{arch, console::Console, logging};

/// Handles a syscall number and a0 argument, returning the scalar a0 result.
///
/// Exit never returns. User pointers are not interpreted. Unknown calls and
/// invalid scalar values return their ABI error without terminating execution.
pub(crate) fn handle(number: usize, argument: usize) -> usize {
  logging::trace!(
    Syscall,
    "User syscall (number={number}, argument={argument:#x})"
  );
  match number {
    user_abi::CONSOLE_WRITE_BYTE => {
      let Ok(byte) = u8::try_from(argument) else {
        logging::debug!(Syscall, "Rejected console byte (value={argument:#x})");
        return user_abi::ERROR_INVALID_ARGUMENT;
      };
      Console::write_byte(byte).map_or(user_abi::ERROR_IO, |()| 0)
    }
    user_abi::TIMER_TICKS => usize::try_from(arch::user_timer_ticks()).unwrap_or(usize::MAX),
    user_abi::EXIT => super::exit(argument),
    _ => {
      logging::debug!(Syscall, "Unsupported user syscall (number={number})");
      user_abi::ERROR_NOT_SUPPORTED
    }
  }
}
