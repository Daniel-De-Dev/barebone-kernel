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

mod address;
mod arch;
mod console;
mod firmware;
mod logging;

use address::PhysAddr;
use core::panic::PanicInfo;
use fdt::Fdt;

/// Runs the kernel after architecture-specific initialization.
///
/// `hart_id` identifies the RISC-V hart on which the kernel was entered,
/// while `dtb` is the physical address of the device tree supplied by the
/// previous firmware stage.
///
/// This function does not return.
#[unsafe(no_mangle)]
extern "C" fn main(hart_id: usize, dtb: usize) -> ! {
  let dtb_phys = PhysAddr::new(dtb);

  logging::info!("kernel entered (hart={}, dtb={:#x})", hart_id, dtb_phys);

  // SAFETY:
  // Address translation has not been enabled, so the physical DTB address
  // supplied by the firmware is directly addressable by the kernel.
  let fdt = unsafe { Fdt::from_ptr(dtb_phys.as_usize() as *const u8) };

  logging::info!("FDT data structure:\n{:#?}", fdt);

  logging::debug!("kernel halting");
  arch::halt()
}

/// Handles unrecoverable Rust panics by halting the current hart.
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
  arch::halt()
}
