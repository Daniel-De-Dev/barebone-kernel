//! RISC-V 64-bit supervisor-mode trap handling.

use core::arch::{asm, global_asm};

use crate::{arch, logging};

// Supervisor-mode trap entry point.
//
// `stvec` is configured in Direct mode, so all supervisor traps transfer
// control here.
//
// The time probe is the only recoverable path. It allows a0, a1, and t0 to be
// clobbered; t1 retains its saved supervisor status. Every other trap reaches
// the Rust handler, which never returns, so those paths do not need to preserve
// interrupted general-purpose registers.
global_asm!(
  r#"
  .section .text
  .align 2  /* 4-byte alignment required by stvec.BASE */
  .global __trap_entry

__trap_entry:
  csrr a0, scause
  li t0, 2  /* Illegal instruction */
  bne a0, t0, .Lfatal_trap

.Lcheck_time_probe:
  csrr a1, sepc
  la t0, {time_probe}
  bne a1, t0, .Lfatal_trap

  la t0, {time_unavailable}
  csrw sepc, t0
  sret

.Lfatal_trap:
  csrr a1, sepc
  csrr a2, stval
  csrr a3, sstatus

  tail {handler}
  "#,
  handler = sym trap_handler,
  time_probe = sym super::time::__probe_time_read,
  time_unavailable = sym super::time::__probe_time_unavailable,
);

unsafe extern "C" {
  /// Assembly entry point installed in `stvec`.
  fn __trap_entry();
}

/// Installs the supervisor-mode trap entry point.
///
/// The address of [`__trap_entry`] is written to `stvec` using Direct mode,
/// causing all supervisor traps to transfer control to the same entry point.
///
/// This function only installs the trap vector.
pub(crate) fn init() {
  #[expect(
    clippy::as_conversions,
    reason = "the assembly trap-entry symbol must be converted to a code pointer so its address can be written to stvec"
  )]
  let address = (__trap_entry as *const ()).addr();

  // `stvec.BASE` must be 4-byte aligned, leaving bits [1:0] available
  // for the MODE field.
  debug_assert_eq!(address & 0b11, 0);

  // SAFETY:
  // `__trap_entry` is statically defined with `.align 2`, which guarantees the
  // 4-byte alignment required by `stvec.BASE`. Its low two address bits are
  // therefore zero, selecting Direct mode when written to `stvec`.
  unsafe {
    asm!(
      "csrw stvec, {address}",
      address = in(reg) address,
      options(nostack),
    );
  }
}

/// Handles traps that are not yet recoverable by the kernel.
///
/// The trap state is provided by `__trap_entry` through the normal RISC-V
/// argument registers. The current implementation only reports the trap and
/// halts.
extern "C" fn trap_handler(scause: usize, sepc: usize, stval: usize, sstatus: usize) -> ! {
  // Avoid counter fault while reporting an unrecoverable trap.
  super::time::disable();

  logging::error!(
    Boot,
    "Unhandled supervisor trap occurred:\n\
      scause:  {:#018x}\n\
      sepc:    {:#018x}\n\
      stval:   {:#018x}\n\
      sstatus: {:#018x}",
    scause,
    sepc,
    stval,
    sstatus,
  );

  arch::halt()
}
