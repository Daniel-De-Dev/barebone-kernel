//! Returning traps on an aligned supervisor stack.
//!
//! Entry preserves integer registers and trap status on the interrupted stack.
//! Floating-point and vector access is disabled while the handler runs, leaving
//! that register state untouched. Handlers run with supervisor interrupts masked.

use core::{
  arch::{asm, global_asm},
  mem::{offset_of, size_of},
};

use crate::{arch, logging};

/// Supervisor origin recorded in `sstatus.SPP`.
const SUPERVISOR_ORIGIN: usize = 1 << 8;

/// Floating-point and vector state fields in `sstatus`.
const EXTENSION_STATE_MASK: usize = (3 << 13) | (3 << 9);

/// Supervisor timer interrupt encoded in `scause`.
const TIMER_INTERRUPT: usize = (1 << 63) | 5;

/// Illegal-instruction exception encoded in `scause`.
const ILLEGAL_INSTRUCTION: usize = 2;

/// Saved supervisor execution state at trap entry.
#[repr(C, align(16))]
struct TrapFrame {
  /// Integer registers indexed by architectural register number.
  registers: [usize; 32],

  /// Instruction address at which execution resumes.
  epc: usize,

  /// Supervisor status captured after hardware trap entry.
  status: usize,

  /// Interrupt or exception code supplied by the processor.
  cause: usize,

  /// Additional exception information supplied by the processor.
  value: usize,
}

// Save every integer register before using it as scratch space. The original
// stack pointer occupies its architectural register slot. Rust receives an
// aligned frame and returns with supervisor interrupts still masked.
global_asm!(
  r#"
  .section .text
  .balign 4
  .global __trap_entry

__trap_entry:
  addi sp, sp, -{frame_size}
  sd t0, {registers}+8*5(sp)
  sd zero, {registers}+8*0(sp)
  sd ra, {registers}+8*1(sp)
  addi t0, sp, {frame_size}
  sd t0, {registers}+8*2(sp)
  sd gp, {registers}+8*3(sp)
  sd tp, {registers}+8*4(sp)
  sd t1, {registers}+8*6(sp)
  sd t2, {registers}+8*7(sp)
  sd s0, {registers}+8*8(sp)
  sd s1, {registers}+8*9(sp)
  sd a0, {registers}+8*10(sp)
  sd a1, {registers}+8*11(sp)
  sd a2, {registers}+8*12(sp)
  sd a3, {registers}+8*13(sp)
  sd a4, {registers}+8*14(sp)
  sd a5, {registers}+8*15(sp)
  sd a6, {registers}+8*16(sp)
  sd a7, {registers}+8*17(sp)
  sd s2, {registers}+8*18(sp)
  sd s3, {registers}+8*19(sp)
  sd s4, {registers}+8*20(sp)
  sd s5, {registers}+8*21(sp)
  sd s6, {registers}+8*22(sp)
  sd s7, {registers}+8*23(sp)
  sd s8, {registers}+8*24(sp)
  sd s9, {registers}+8*25(sp)
  sd s10, {registers}+8*26(sp)
  sd s11, {registers}+8*27(sp)
  sd t3, {registers}+8*28(sp)
  sd t4, {registers}+8*29(sp)
  sd t5, {registers}+8*30(sp)
  sd t6, {registers}+8*31(sp)

  csrr t0, sepc
  sd t0, {epc}(sp)
  csrr t0, sstatus
  sd t0, {status}(sp)
  csrr t0, scause
  sd t0, {cause}(sp)
  csrr t0, stval
  sd t0, {value}(sp)

  li t0, {extension_state_mask}
  csrc sstatus, t0

  .option push
  .option norelax
  la gp, __global_pointer$
  .option pop

  mv a0, sp
  call {handler}

  ld t0, {epc}(sp)
  csrw sepc, t0
  ld t0, {status}(sp)
  andi t0, t0, -3
  csrw sstatus, t0

  ld ra, {registers}+8*1(sp)
  ld gp, {registers}+8*3(sp)
  ld tp, {registers}+8*4(sp)
  ld t1, {registers}+8*6(sp)
  ld t2, {registers}+8*7(sp)
  ld s0, {registers}+8*8(sp)
  ld s1, {registers}+8*9(sp)
  ld a0, {registers}+8*10(sp)
  ld a1, {registers}+8*11(sp)
  ld a2, {registers}+8*12(sp)
  ld a3, {registers}+8*13(sp)
  ld a4, {registers}+8*14(sp)
  ld a5, {registers}+8*15(sp)
  ld a6, {registers}+8*16(sp)
  ld a7, {registers}+8*17(sp)
  ld s2, {registers}+8*18(sp)
  ld s3, {registers}+8*19(sp)
  ld s4, {registers}+8*20(sp)
  ld s5, {registers}+8*21(sp)
  ld s6, {registers}+8*22(sp)
  ld s7, {registers}+8*23(sp)
  ld s8, {registers}+8*24(sp)
  ld s9, {registers}+8*25(sp)
  ld s10, {registers}+8*26(sp)
  ld s11, {registers}+8*27(sp)
  ld t3, {registers}+8*28(sp)
  ld t4, {registers}+8*29(sp)
  ld t5, {registers}+8*30(sp)
  ld t6, {registers}+8*31(sp)
  ld t0, {registers}+8*5(sp)
  ld sp, {registers}+8*2(sp)
  sret
  "#,
  frame_size = const size_of::<TrapFrame>(),
  registers = const offset_of!(TrapFrame, registers),
  epc = const offset_of!(TrapFrame, epc),
  status = const offset_of!(TrapFrame, status),
  cause = const offset_of!(TrapFrame, cause),
  value = const offset_of!(TrapFrame, value),
  extension_state_mask = const EXTENSION_STATE_MASK,
  handler = sym trap_handler,
);

unsafe extern "C" {
  /// Assembly entry point installed in `stvec`.
  fn __trap_entry();
}

/// Installs a direct supervisor trap vector and masks local interrupt sources.
///
/// Traps must originate in supervisor mode with an ABI-aligned, writable stack
/// and enough remaining space for a saved frame and the Rust handler.
pub(crate) fn init() {
  #[expect(
    clippy::as_conversions,
    reason = "the assembly symbol must be converted to a code pointer for stvec"
  )]
  let address = (__trap_entry as *const ()).addr();

  debug_assert_eq!(address & 0b11, 0);

  // SAFETY:
  // The code address is aligned for Direct mode. Clearing sie masks supervisor
  // interrupt sources before the local trap destination is replaced.
  unsafe {
    asm!(
      "csrw sie, zero",
      "csrw stvec, {address}",
      address = in(reg) address,
      options(nostack),
    );
  }

  logging::debug!(
    Trap,
    "Supervisor trap vector installed (stvec={address:#x}, mode=direct)"
  );
}

/// Handles recoverable supervisor traps or reports an unrecoverable fault.
#[expect(
  clippy::as_conversions,
  reason = "assembly labels are compared and stored only as instruction addresses"
)]
extern "C" fn trap_handler(frame: &mut TrapFrame) {
  if frame.status & SUPERVISOR_ORIGIN == 0 {
    fatal(frame);
  }

  if frame.cause == ILLEGAL_INSTRUCTION
    && frame.epc == (super::time::__probe_time_read as *const ()).addr()
  {
    frame.epc = (super::time::__probe_time_unavailable as *const ()).addr();
    return;
  }

  if frame.cause == TIMER_INTERRUPT {
    if let Err(error) = super::timer::handle_interrupt() {
      logging::error!(Timer, "Supervisor timer failed ({error:?})");
      fatal(frame);
    }
    return;
  }

  fatal(frame);
}

/// Disables counter reads, reports saved trap state, and parks the hart.
fn fatal(frame: &TrapFrame) -> ! {
  super::time::disable();

  logging::error!(
    Trap,
    "Unhandled supervisor trap occurred:\n\
      scause:  {:#018x}\n\
      sepc:    {:#018x}\n\
      stval:   {:#018x}\n\
      sstatus: {:#018x}",
    frame.cause,
    frame.epc,
    frame.value,
    frame.status,
  );

  arch::halt()
}
