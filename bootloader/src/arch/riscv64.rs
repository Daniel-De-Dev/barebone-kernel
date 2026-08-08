use core::arch::{asm, global_asm};

global_asm!(
  r#"
    .section .text.init
    .global _start

_start:
    .option push
    .option norelax

    /* Initialize gp */
    la gp, __global_pointer$

    .option pop

    /* Initilize stack */
    la sp, _stack_end

    /* Clear .bss */
    la t0, _bss_start
    la t1, _bss_end

.bss_loop:
    bgeu t0, t1, .bss_done

    sd zero, 0(t0)
    addi t0, t0, 8

    j .bss_loop

.bss_done:
    tail main
  "#
);

pub fn halt() -> ! {
  loop {
    unsafe {
      asm!("wfi", options(nomem, nostack));
    }
  }
}
