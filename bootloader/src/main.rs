#![no_std]
#![no_main]

use core::arch::global_asm;
use core::panic::PanicInfo;

global_asm!(
  r#"
    .section .text.init
    .global _start
    _start:
        la sp, _stack_end
        j main
    "#
);

#[unsafe(no_mangle)]
pub extern "C" fn main(hart_id: usize, dtb: usize) -> ! {
  for byte in b"Hello from S-mode!\n" {
    // TODO: check version and read on crate
    let _ = sbi_rt::console_write_byte(*byte);
  }

  loop {
    unsafe {
      core::arch::asm!("wfi");
    }
  }
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
  loop {
    unsafe {
      core::arch::asm!("wfi");
    }
  }
}
