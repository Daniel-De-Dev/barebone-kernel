//! Bare-metal kernel entry point.
//!
//! This crate coordinates the boot flow after the target-specific entry code
//! has established a valid Rust execution environment.
//!
//! Architecture-specific startup and CPU primitives are provided by [`arch`],
//! while kernel logging is provided by [`logging`].
//!
//! The RISC-V entry code transfers control to [`main`] with the boot hart ID
//! and device-tree address supplied by the previous firmware stage.

#![no_std]
#![no_main]

mod arch;
mod console;
mod firmware;
mod logging;

use core::panic::PanicInfo;
use fdt::header::Header;

/// Runs the kernel after architecture-specific initialization.
///
/// `hart_id` identifies the RISC-V hart on which the kernel was entered,
/// while `dtb` is the physical address of the device tree supplied by the
/// previous firmware stage.
///
/// This function does not return.
// TODO: Consider representing the DTB address with a physical-address type.
#[unsafe(no_mangle)]
extern "C" fn main(hart_id: usize, dtb: usize) -> ! {
  logging::info!("kernel entered (hart={}, dtb={:#x})", hart_id, dtb);

  // Cast the physical address provided by the firmware into a raw pointer
  let dtb_ptr = dtb as *const [u8; 40];

  // Safety: The firmware guarantees `dtb` points to a valid Flattened Devicetree in memory.
  let header_bytes = unsafe { &*dtb_ptr };

  match Header::parse(header_bytes) {
    Ok(header) => logging::info!("FDT Header: {:#?}", header),
    Err(e) => logging::info!("Failed to parse FDT header: {:?}", e),
  }

  logging::debug!("kernel halting");
  arch::halt()
}

/// Handles unrecoverable Rust panics by halting the current hart.
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
  arch::halt()
}
