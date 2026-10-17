//! Scoped control of supervisor interrupt delivery.

use core::{arch::asm, marker::PhantomData};

/// Restores the previous supervisor interrupt-enable state when dropped.
///
/// Each guard restores the state it observed. Drop nested guards in reverse
/// acquisition order to keep interrupts disabled until the outer guard drops.
/// The guard must be dropped on the hart where it was acquired.
#[must_use = "dropping the guard restores the previous interrupt-enable state"]
pub(crate) struct InterruptGuard {
  /// Whether the SIE bit was set before acquisition.
  was_enabled: bool,

  /// Makes the guard neither `Send` nor `Sync`.
  _hart: PhantomData<*mut ()>,
}

/// Clears supervisor interrupt enable and returns a restoration guard.
///
/// This controls local interrupt delivery only. It does not prevent synchronous
/// exceptions or stop other harts. The assembly also orders surrounding
/// compiler memory accesses across the interrupt-state change.
pub(crate) fn disable() -> InterruptGuard {
  let status: usize;

  // SAFETY:
  // The kernel executes in supervisor mode. This reads sstatus and clears only
  // its SIE bit, preserving all other status bits and Rust-managed memory.
  unsafe {
    asm!("csrrci {status}, sstatus, 2", status = out(reg) status, options(nostack));
  }

  InterruptGuard {
    was_enabled: status & 2 != 0,
    _hart: PhantomData,
  }
}

impl Drop for InterruptGuard {
  fn drop(&mut self) {
    if self.was_enabled {
      // SAFETY:
      // This guard stays on its acquiring hart. Setting SIE restores its saved
      // enabled state without changing any other status bit or memory.
      unsafe {
        asm!("csrsi sstatus, 2", options(nostack));
      }
    } else {
      // SAFETY:
      // This guard stays on its acquiring hart. Clearing SIE restores its saved
      // disabled state without changing any other status bit or memory.
      unsafe {
        asm!("csrci sstatus, 2", options(nostack));
      }
    }
  }
}
