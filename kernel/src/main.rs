//! Bare-metal kernel
//!
//! This crate implements bare essentials to have a proof of concept kernel
//! compile and run. Mainly to begin targeting running on RISC-V 64-bit
//! architecture with SBI. But effort will be made to allow for modularity,
//! where implementations could be swapped out easily.

#![no_std]
#![no_main]

mod arch;
mod console;
mod firmware;
mod logging;
mod memory;

use core::panic::PanicInfo;
use fdt::Fdt;
use memory::{BootFrameAllocator, PhysAddr, PhysRange};

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
  let fdt = match unsafe { Fdt::from_ptr(dtb_ptr) } {
    Ok(fdt) => fdt,
    Err(error) => {
      logging::error!("Failed to parse FDT ({error:?}); kernel startup is unrecoverable, halting");

      arch::halt();
    }
  };

  logging::debug!("FDT data structure:\n{:#?}", fdt);

  for memory_range in fdt.memory_ranges() {
    logging::debug!("{:?}", memory_range);
  }

  for memory_reservation in fdt.reserved_memory_ranges() {
    logging::debug!("{:?}", memory_reservation);
  }

  for memory_reservation in fdt.memory_reservations() {
    logging::debug!("{:?}", memory_reservation);
  }

  let kernel_range = memory::kernel_range();

  logging::debug!("Kernel Range: {:?}", kernel_range);

  let Some(dtb_range) = PhysRange::from_start_size(dtb_phys, fdt.total_size()) else {
    logging::error!(
      "Failed to establish DTB physical range; \
      kernel startup is unrecoverable, halting"
    );

    arch::halt();
  };

  logging::debug!("DTB Range: {:?}", dtb_range);

  let frames = match BootFrameAllocator::new(&fdt, kernel_range, dtb_range) {
    Ok(frames) => frames,
    Err(error) => {
      logging::error!(
        "Failed to initialize physical frame allocator ({error:?}); \
        kernel startup is unrecoverable, halting"
      );

      arch::halt();
    }
  };

  logging::debug!("kernel halting");
  arch::halt()
}

/// Handles unrecoverable Rust panics by halting the current hart.
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
  arch::halt()
}
