#![no_std]
#![no_main]

mod arch;

use core::panic::PanicInfo;

#[unsafe(no_mangle)]
pub extern "C" fn main(_hart_id: usize, _dtb: usize) -> ! {
  for byte in b"Hello from S-mode!\n" {
    // TODO: check version and read on crate
    let _ = sbi_rt::console_write_byte(*byte);
  }

  arch::halt()
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
  arch::halt()
}
