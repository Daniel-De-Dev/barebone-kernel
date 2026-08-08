#![no_std]
#![no_main]

mod arch;
mod logging;

use core::panic::PanicInfo;

#[unsafe(no_mangle)]
pub extern "C" fn main(hart_id: usize, dtb: usize) -> ! {
  logging::init().expect("logger already initialized");

  log::debug!("debug logging initialized");
  log::info!("bootloader entered (hart={}, dtb={:#x})", hart_id, dtb,);

  log::debug!("Bootloader halting");
  arch::halt()
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
  arch::halt()
}
