//! Bare-metal kernel
//!
//! This crate implements bare essentials to have a proof of concept kernel
//! compile and run. Mainly to begin targeting running on RISC-V 64-bit
//! architecture with SBI. But effort will be made to allow for modularity,
//! where implementations could be swapped out easily.

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

  logging::info!("initializing trap handling");
  arch::init_trap();

  let dtb_ptr = core::ptr::with_exposed_provenance::<u8>(dtb_phys.as_usize());

  // SAFETY:
  // Address translation is not enabled, so the firmware-provided physical DTB
  // address is directly addressable by the kernel. The boot environment
  // guarantees that it points to a readable, contiguous DTB memory that remains
  // valid while it is being parsed.
  let fdt = unsafe { Fdt::from_ptr(dtb_ptr) };

  logging::debug!("FDT data structure:\n{:#?}", fdt);

  logging::debug!("kernel halting");
  arch::halt()
}

/// Handles unrecoverable Rust panics by halting the current hart.
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
  arch::halt()
}
