//! Optional access to the RV64 time counter for log timestamps.
//!
//! Initialization tries one `rdtime`. The trap entry recovers only an
//! illegal-instruction exception at that exact instruction; every other
//! trap keeps its normal fatal path.
//!
//! Availability is cached for the current single-hart kernel. Firmware must
//! preserve counter access after a successful probe. Supporting other harts
//! requires probing and retaining availability separately for each hart.
//! Values are raw platform timebase ticks, not CPU cycles or wall-clock time.

use core::{
  arch::{asm, global_asm},
  sync::atomic::{AtomicU8, Ordering},
};

/// Timer has not been initialized.
const UNINITIALIZED: u8 = 0;

/// Counter reads are disabled.
const UNAVAILABLE: u8 = 1;

/// Timer is available and can be used.
const AVAILABLE: u8 = 2;

/// Atomic state holder which decides if timer is available or not.
static STATE: AtomicU8 = AtomicU8::new(UNINITIALIZED);

// The recoverable trap may clobber a0, a1, and t0, which are caller-saved.
// It preserves t1 holding the original sstatus, ra, and all callee-saved
// registers. Interrupts are disabled during the probe, and either return path
// restores the original supervisor status.
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

/// Probes time access once, before the first timestamped log record.
///
/// Repeated calls retain the initial result. A denied or missing counter
/// leaves later log records free of counter reads.
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

/// Suppresses counter reads before a fatal trap reports its diagnostics.
pub(super) fn disable() {
  STATE.store(UNAVAILABLE, Ordering::Relaxed);
}
