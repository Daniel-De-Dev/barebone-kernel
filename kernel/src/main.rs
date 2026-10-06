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
use memory::{BootFrameAllocator, PhysAddr, PhysRange, kernel_sections};

// TODO: add debug/trace info for allocations
/// Runs the kernel after architecture-specific initialization.
///
/// `hart_id` identifies the RISC-V hart on which the kernel was entered.
/// `dtb` is the physical address of the device tree supplied by the previous
/// firmware stage. `kernel_phys_start` is the physical start of the kernel
/// image preserved by the bootstrap before entering the higher half.
///
/// This function does not return.
#[unsafe(no_mangle)]
extern "C" fn main(hart_id: usize, dtb: usize, kernel_phys_start: usize) -> ! {
  let dtb_phys = PhysAddr::new(dtb);
  let kernel_phys_start = PhysAddr::new(kernel_phys_start);

  logging::info!(
    "kernel entered (hart={}, dtb={:#x}, kernel_start={:#x})",
    hart_id,
    dtb_phys,
    kernel_phys_start
  );

  logging::info!("initializing trap handling");
  arch::init_trap();

  // SAFETY:
  // Architecture bootstrap enters `main` with the complete active Sv39 root
  // page table writably identity-mapped. No other hart or code modifies the
  // paging entries reserved for bootstrap use while this capability exists.
  let mut bootstrap_paging = match unsafe { arch::BootstrapPaging::claim() } {
    Ok(paging) => paging,
    Err(error) => {
      logging::error!(
        "Failed to claim bootstrap paging state ({error:?}); \
        kernel startup is unrecoverable, halting"
      );

      arch::halt();
    }
  };

  let dtb_virtual = match bootstrap_paging.map_fdt(dtb_phys) {
    Ok(address) => address,
    Err(error) => {
      logging::error!(
        "Failed to establish higher-half DTB mapping ({error:?}); \
         kernel startup is unrecoverable, halting"
      );

      arch::halt();
    }
  };

  logging::debug!("DTB higher-half alias: {:#x}", dtb_virtual.as_usize(),);

  let dtb_ptr = core::ptr::with_exposed_provenance::<u8>(dtb_virtual.as_usize());

  // SAFETY:
  // `BootstrapPaging::map_fdt` established a readable mapping containing the
  // complete DTB. `into_kernel` transition preserves all pages backing this
  // blob at the same virtual alias.
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

  let Some(kernel_range) = memory::kernel_range(kernel_phys_start) else {
    logging::error!("Invalid physical kernel range; kernel startup is unrecoverable, halting");

    arch::halt();
  };

  let kernel_sections = kernel_sections();

  logging::debug!("{:#?}", kernel_sections);

  logging::debug!("Kernel Range: {:?}", kernel_range);

  let Some(dtb_range) = PhysRange::from_start_size(dtb_phys, fdt.total_size()) else {
    logging::error!(
      "Failed to establish DTB physical range; kernel startup is unrecoverable, halting"
    );

    arch::halt();
  };

  logging::debug!("DTB Range: {:?}", dtb_range);

  // SAFETY:
  // The architecture bootstrap preserves the actual physical start of the live
  // kernel image, so `kernel_range` covers its complete linker-defined boot
  // footprint. `dtb_range` covers the same firmware-provided DTB backing `fdt`.
  // The FDT is trusted to describe all other physical memory that is unavailable
  // for allocation through its reservation information.
  let mut frames = match unsafe { BootFrameAllocator::claim(&fdt, kernel_range, dtb_range) } {
    Ok(frames) => frames,
    Err(error) => {
      logging::error!(
        "Failed to claim physical frame allocator ({error:?}); \
        kernel startup is unrecoverable, halting"
      );

      arch::halt();
    }
  };

  // SAFETY:
  // These sections come directly from the live kernel's linker symbols, and
  // bootstrap supplied the actual physical load address. They cover the live
  // code, trap vector, constants, data, and stack. `dtb_range` and `dtb_virtual`
  // describe the complete blob backing `fdt` and the frame allocator. No other
  // retained reference depends on a bootstrap-only mapping. Startup relies on
  // the boot environment entering this hart with supervisor interrupts disabled
  // and does not enable them before this transition. No other hart accesses
  // these paging structures.
  let kernel_paging = match unsafe {
    bootstrap_paging.into_kernel(
      kernel_phys_start,
      &kernel_sections,
      dtb_virtual,
      dtb_range,
      &mut frames,
    )
  } {
    Ok(paging) => paging,
    Err(error) => {
      logging::error!(
        "Failed to enter kernel paging ({error:?}); \
        kernel startup is unrecoverable, halting"
      );

      arch::halt();
    }
  };

  logging::info!("Kernel paging active");
  logging::debug!("{:#?}", kernel_paging);

  logging::info!("kernel halting");
  arch::halt()
}

/// Handles unrecoverable Rust panics by halting the current hart.
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
  arch::halt()
}
