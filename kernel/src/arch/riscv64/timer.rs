//! Periodic supervisor timer delivery through SBI TIME.
//!
//! Deadlines are relative to the counter observed when each event is handled.

use core::{
  arch::asm,
  num::{NonZeroU32, NonZeroU64},
  sync::atomic::{AtomicU64, Ordering},
  time::Duration,
};

use crate::{firmware::sbi, logging};

/// Supervisor timer interrupt-enable bit in `sie`.
const TIMER_ENABLE: usize = 1 << 5;

/// Configured interval in time-counter ticks, or zero before initialization.
static INTERVAL: AtomicU64 = AtomicU64::new(0);

/// Number of successfully handled timer interrupts, wrapping at `u64::MAX`.
static TICKS: AtomicU64 = AtomicU64::new(0);

/// Failures while configuring or servicing a periodic timer.
#[derive(Debug)]
pub(crate) enum TimerError {
  /// A periodic timer is already configured.
  AlreadyInitialized,

  /// An interrupt arrived before an interval was configured.
  NotInitialized,

  /// The time counter is unavailable on this hart.
  CounterUnavailable,

  /// The duration is zero or its tick count cannot be represented.
  InvalidInterval,

  /// The next absolute deadline would overflow the time counter.
  DeadlineOverflow,

  /// Firmware rejected timer programming.
  Firmware(sbi::SetTimerError),
}

/// Arms a periodic timer and unmasks its local interrupt source.
///
/// Global supervisor interrupt delivery retains its previous state. Failed
/// first initialization does not install an interval or unmask the source.
/// Repeated initialization leaves the existing configuration unchanged.
///
/// # Safety
///
/// Call only on the hart whose supervisor trap vector handles this timer.
/// This hart must be the sole caller and remain the only user of this timer
/// state. This driver must exclusively control the timer deadline and its
/// interrupt-enable bit.
/// Counter access must remain available after its successful probe.
///
/// # Errors
///
/// Returns an error for repeated initialization, unavailable counter access,
/// an invalid interval, deadline overflow, or rejected firmware calls.
pub(crate) unsafe fn init(timebase_hz: NonZeroU32, period: Duration) -> Result<(), TimerError> {
  let _interrupts = super::interrupt::disable();

  if INTERVAL.load(Ordering::Relaxed) != 0 {
    return Err(TimerError::AlreadyInitialized);
  }

  let interval = interval_ticks(timebase_hz, period)?;
  let deadline = next_deadline(interval.get())?;
  sbi::set_timer(deadline).map_err(TimerError::Firmware)?;

  INTERVAL.store(interval.get(), Ordering::Relaxed);

  // SAFETY:
  // The handler's interval and the first deadline are installed while local
  // supervisor delivery is masked. Only the timer source is unmasked here.
  unsafe {
    asm!("csrs sie, {mask}", mask = in(reg) TIMER_ENABLE, options(nostack));
  }

  logging::debug!(
    Timer,
    "Periodic timer configured (interval_ticks={}, first_deadline={deadline})",
    interval.get(),
  );

  Ok(())
}

/// Returns the wrapping count of successfully handled timer interrupts.
pub(crate) fn ticks() -> u64 {
  TICKS.load(Ordering::Relaxed)
}

/// Rearms the periodic timer and records one delivered interrupt.
///
/// Called on the timer's owning hart with supervisor interrupts masked.
///
/// # Errors
///
/// Returns an error for missing initialization, unavailable counter access,
/// deadline overflow, or rejected firmware calls.
pub(super) fn handle_interrupt() -> Result<(), TimerError> {
  let interval = INTERVAL.load(Ordering::Relaxed);
  if interval == 0 {
    return Err(TimerError::NotInitialized);
  }

  let deadline = next_deadline(interval)?;
  sbi::set_timer(deadline).map_err(TimerError::Firmware)?;
  TICKS.fetch_add(1, Ordering::Relaxed);

  Ok(())
}

/// Converts a duration to a non-zero tick count, rounding upward.
///
/// # Errors
///
/// Returns an error for zero duration or an unrepresentable tick count.
fn interval_ticks(timebase_hz: NonZeroU32, period: Duration) -> Result<NonZeroU64, TimerError> {
  let scaled = u128::from(timebase_hz.get())
    .checked_mul(period.as_nanos())
    .ok_or(TimerError::InvalidInterval)?;
  let ticks = scaled.div_ceil(1_000_000_000);
  let ticks = u64::try_from(ticks).map_err(|_| TimerError::InvalidInterval)?;

  NonZeroU64::new(ticks).ok_or(TimerError::InvalidInterval)
}

/// Computes a future deadline from the current time counter.
///
/// # Errors
///
/// Returns an error if the counter is unavailable or addition would overflow.
fn next_deadline(interval: u64) -> Result<u64, TimerError> {
  super::time::ticks()
    .ok_or(TimerError::CounterUnavailable)?
    .checked_add(interval)
    .ok_or(TimerError::DeadlineOverflow)
}
