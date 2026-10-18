//! Returning traps and integer-only user entry on aligned supervisor stacks.
//!
//! User traps select a supervisor stack through sscratch before storing state.
//! Supervisor execution keeps sscratch zero and uses its interrupted stack.
//! Floating-point and vector access is disabled while the handler runs, leaving
//! that register state untouched. Handlers run with supervisor interrupts masked.

use core::{
  arch::{asm, global_asm},
  mem::{offset_of, size_of},
  ptr,
  sync::atomic::{AtomicU64, Ordering},
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

/// Environment call raised from user mode.
const USER_ECALL: usize = 8;

/// RV64 user status with supervisor interrupts enabled on privilege return.
const INITIAL_USER_STATUS: usize = (2 << 32) | (1 << 5);

/// Wrapping count of timer interrupts that interrupted user execution.
static USER_TICKS: AtomicU64 = AtomicU64::new(0);

/// Saved integer execution state and supervisor trap metadata.
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

/// Integer-only execution state for the first return into user mode.
pub(crate) struct UserContext {
  /// Registers and privilege-return state consumed by the trap restore path.
  frame: TrapFrame,
}

impl UserContext {
  /// Prepares zeroed integer registers, an entry address, and a user stack.
  ///
  /// Floating-point and vector access start disabled. Mapping validity is
  /// checked by the caller before this context is entered.
  pub(crate) const fn new(entry: crate::memory::VirtAddr, stack: crate::memory::VirtAddr) -> Self {
    let mut registers = [0; 32];
    registers[2] = stack.as_usize();
    Self {
      frame: TrapFrame {
        registers,
        epc: entry.as_usize(),
        status: INITIAL_USER_STATUS,
        cause: 0,
        value: 0,
      },
    }
  }
}

// Stack selection uses only sp and sscratch, preserving every other register.
// Once the interrupted sp is saved, sscratch is cleared so synchronous traps
// in the handler retain the current supervisor stack.
global_asm!(
  r#"
  .section .text
  .balign 4
  .global __trap_entry

__trap_entry:
  csrrw sp, sscratch, sp
  bnez sp, .Luser_stack
  csrrw sp, sscratch, sp
  addi sp, sp, -{frame_size}
  sd t0, {registers}+8*5(sp)
  addi t0, sp, {frame_size}
  j .Lsave_context
.Luser_stack:
  addi sp, sp, -{frame_size}
  sd t0, {registers}+8*5(sp)
  csrr t0, sscratch
  csrw sscratch, zero
.Lsave_context:
  sd t0, {registers}+8*2(sp)
  sd zero, {registers}+8*0(sp)
  sd ra, {registers}+8*1(sp)
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
  mv tp, zero

  mv a0, sp
  call {handler}

  .global __trap_restore
__trap_restore:
  ld t0, {epc}(sp)
  csrw sepc, t0
  ld t0, {status}(sp)
  andi t0, t0, -3
  csrw sstatus, t0

  csrw sscratch, zero
  andi t0, t0, {supervisor_origin}
  bnez t0, .Lrestore_registers
  addi t0, sp, {frame_size}
  csrw sscratch, t0
.Lrestore_registers:

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
  supervisor_origin = const SUPERVISOR_ORIGIN,
  handler = sym trap_handler,
);

unsafe extern "C" {
  /// Assembly entry point installed in `stvec`.
  fn __trap_entry();

  /// Restores a `TrapFrame` addressed by sp and returns through sret.
  fn __trap_restore();
}

/// Enters user execution using a permanently retained supervisor trap stack.
///
/// Instruction fetch is synchronized after loading executable bytes. Local
/// supervisor delivery is masked before selecting the user restore frame.
///
/// # Safety
///
/// The context's entry must be mapped executable for user mode and its stack
/// must be writable user memory. `kernel_stack_top` must be 16-byte-aligned
/// above a sufficiently sized, exclusively owned, permanently writable
/// supervisor stack. Its top frame must fit and have no live references.
/// The installed trap vector and every enabled interrupt handler must be ready
/// on this hart, with the corresponding page-table root active.
pub(crate) unsafe fn enter_user(
  context: UserContext,
  kernel_stack_top: crate::memory::VirtAddr,
) -> ! {
  let frame = ptr::with_exposed_provenance_mut::<TrapFrame>(
    kernel_stack_top
      .as_usize()
      .wrapping_sub(size_of::<TrapFrame>()),
  );
  // SAFETY:
  // The caller supplies exclusive, aligned stack storage covering this frame.
  // Every field is initialized, and no reference survives the transfer.
  unsafe {
    ptr::write(frame, context.frame);
  }

  // SAFETY:
  // The caller established user mappings and interrupt readiness. The complete
  // saved frame occupies the supplied supervisor stack. The non-returning jump
  // consumes that frame through the common integer-register restore path.
  unsafe {
    asm!(
      "csrci sstatus, 2",
      "fence.i",
      "mv sp, {frame}",
      "j {restore}",
      frame = in(reg) frame,
      restore = sym __trap_restore,
      options(noreturn),
    );
  }
}

/// Returns the wrapping number of timer interrupts delivered from user mode.
pub(crate) fn user_timer_ticks() -> u64 {
  USER_TICKS.load(Ordering::Relaxed)
}

/// Installs a direct supervisor trap vector and masks local interrupt sources.
///
/// Supervisor traps require an aligned, writable stack with space for the saved
/// frame and handler. User entry must supply a separate supervisor stack.
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
      "csrw sscratch, zero",
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

/// Dispatches timer interrupts, scalar user syscalls, and fatal exceptions.
#[expect(
  clippy::as_conversions,
  reason = "assembly labels are compared and stored only as instruction addresses"
)]
extern "C" fn trap_handler(frame: &mut TrapFrame) {
  let supervisor = frame.status & SUPERVISOR_ORIGIN != 0;

  if supervisor
    && frame.cause == ILLEGAL_INSTRUCTION
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
    if !supervisor {
      USER_TICKS.fetch_add(1, Ordering::Relaxed);
    }
    return;
  }

  if !supervisor {
    if frame.cause == USER_ECALL {
      // ecall occupies four bytes.
      let Some(next) = frame.epc.checked_add(4) else {
        crate::process::fault(frame.cause, frame.epc, frame.value);
      };
      frame.epc = next;
      frame.registers[10] =
        crate::process::handle_syscall(frame.registers[17], frame.registers[10]);
      return;
    }
    crate::process::fault(frame.cause, frame.epc, frame.value);
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
