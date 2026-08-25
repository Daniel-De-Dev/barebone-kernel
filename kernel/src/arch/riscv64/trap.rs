//! RISC-V 64-bit supervisor-mode trap handling.

use core::arch::{asm, global_asm};

use crate::{arch, logging};

// Supervisor-mode trap entry point.
//
// `stvec` is configured in Direct mode, so all supervisor traps transfer
// control here.
//
// The Rust handler never returns, so the interrupted general-purpose registers
// do not need to be preserved by this minimal implementation.
global_asm!(
  r#"
  .section .text
  .align 2  /* 4-byte alignment required by stvec.BASE */
  .global __trap_entry

__trap_entry:
  csrr a0, scause
  csrr a1, sepc
  csrr a2, stval
  csrr a3, sstatus

  tail {handler}
  "#,
  handler = sym trap_handler,
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
  let address = __trap_entry as *const () as usize;

  // `stvec.BASE` must be 4-byte aligned, leaving bits [1:0] available
  // for the MODE field.
  debug_assert_eq!(address & 0b11, 0);

  // SAFETY:
  // `address` refers to the statically defined `__trap_entry` routine and
  // satisfies the alignment requirements of `stvec.BASE`.
  //
  // Bits [1:0] are zero, selecting Direct mode.
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
  logging::error!(
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
