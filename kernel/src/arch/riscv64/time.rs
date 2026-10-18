//! Optional access to the RV64 time counter for log timestamps.
//!
//! Initialization probes `rdtime` once. A denied read leaves the counter
//! unavailable to later callers.
//!
//! Availability belongs to the hart that performs the probe. Initialization
//! and reads must stay on that hart. Firmware must preserve access after a
//! successful probe. Values are raw platform timebase ticks.

use core::{
  arch::{asm, global_asm},
  sync::atomic::{AtomicU8, Ordering},
};

/// Counter access has not been probed.
const UNINITIALIZED: u8 = 0;

/// Counter reads are disabled.
const UNAVAILABLE: u8 = 1;

/// Counter reads are available.
const AVAILABLE: u8 = 2;

/// Availability of counter reads on the probed hart.
static STATE: AtomicU8 = AtomicU8::new(UNINITIALIZED);

// Interrupts are disabled during the probe. Both return paths restore the
// original supervisor status retained in t1.
global_asm!(
  r#"
  .section .text
  .align 2
  .global __probe_time
  .global __probe_time_read
  .global __probe_time_unavailable

__probe_time:
  csrrci t1, sstatus, 2

__probe_time_read:
  rdtime a1
  li a0, 1
  j .Lrestore_and_return

__probe_time_unavailable:
  li a0, 0

.Lrestore_and_return:
  csrw sstatus, t1
  ret
  "#,
);

unsafe extern "C" {
  /// Returns one if `rdtime` completes, or zero after a recovered exception.
  fn __probe_time() -> usize;

  /// Exact instruction at which the trap entry may recover denied access.
  pub(super) fn __probe_time_read();

  /// Recovery point returning zero without executing another counter read.
  pub(super) fn __probe_time_unavailable();
}

/// Probes counter access once and retains the first result.
///
/// Repeated calls retain the initial result. A denied or missing counter
/// leaves reads unavailable.
///
/// # Safety
///
/// The current hart must have this kernel's supervisor trap entry installed.
/// Initialization and subsequent reads are restricted to this hart, and
/// firmware must preserve counter access after a successful probe.
pub(crate) unsafe fn init() {
  if STATE
    .compare_exchange(
      UNINITIALIZED,
      UNAVAILABLE,
      Ordering::Relaxed,
      Ordering::Relaxed,
    )
    .is_err()
  {
    return;
  }

  // SAFETY:
  // The caller installed the trap entry that recognizes the probe instruction.
  // The assembly routine follows the C ABI and restores supervisor status on
  // either return path. A failed read is recovered before returning to Rust.
  let available = unsafe { __probe_time() };

  if available != 0 {
    STATE.store(AVAILABLE, Ordering::Relaxed);
  }
}

/// Reads raw timebase ticks only after the current hart's successful probe.
pub(crate) fn ticks() -> Option<u64> {
  if STATE.load(Ordering::Relaxed) != AVAILABLE {
    return None;
  }

  let ticks;

  // SAFETY:
  // Initialization established time access on this hart, which firmware must
  // preserve. The instruction only reads a CSR into the declared register.
  unsafe {
    asm!("rdtime {ticks}", ticks = out(reg) ticks, options(nomem, nostack));
  }

  Some(ticks)
}

/// Marks counter access unavailable for all subsequent reads.
pub(super) fn disable() {
  STATE.store(UNAVAILABLE, Ordering::Relaxed);
}
