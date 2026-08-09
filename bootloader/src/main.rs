//! Bare-metal bootloader entry point.
//!
//! This crate coordinates the boot flow after the target-specific entry code
//! has established a valid Rust execution environment.
//!
//! Architecture-specific startup and CPU primitives are provided by [`arch`],
//! while bootloader logging is provided by [`logging`].
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

/// Runs the bootloader after architecture-specific initialization.
///
/// `hart_id` identifies the RISC-V hart on which the bootloader was entered,
/// while `dtb` is the physical address of the device tree supplied by the
/// previous firmware stage.
///
/// This function does not return.
// TODO: Consider representing the DTB address with a physical-address type.
#[unsafe(no_mangle)]
extern "C" fn main(hart_id: usize, dtb: usize) -> ! {
  logging::info!("bootloader entered (hart={}, dtb={:#x})", hart_id, dtb);

  logging::debug!("bootloader halting");
  arch::halt()
}

/// Handles unrecoverable Rust panics by halting the current hart.
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
  arch::halt()
}
