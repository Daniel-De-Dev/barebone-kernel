//! Bare-metal kernel
//!
//! This crate implements bare essentials to have a proof of concept kernel
//! compile and run. Mainly to begin targeting running on RISC-V 64-bit
//! architecture with SBI. But effort will be made to allow for modularity,
//! where implementations could be swapped out easily.

#![no_std]
#![no_main]
#![feature(alloc_error_handler)]

extern crate alloc;

mod arch;
mod console;
mod firmware;
mod logging;
mod memory;
mod process;

use core::{alloc::Layout, num::NonZeroU32, panic::PanicInfo, time::Duration};

use fdt::Fdt;
use memory::{BootFrameAllocator, PhysAddr, PhysRange, kernel_sections};

/// Runs the kernel after architecture-specific initialization.
///
/// `hart_id` identifies the RISC-V hart on which the kernel was entered.
/// `dtb` is the physical address of the device tree supplied by the previous
/// firmware stage.
///
/// This function does not return.
#[unsafe(no_mangle)]
extern "C" fn main(hart_id: usize, dtb: usize) -> ! {
  let boot_interrupts = arch::disable_interrupts();

  let dtb_phys = PhysAddr::new(dtb);
  let kernel_phys_start = memory::kernel_physical_start();

  arch::init_trap();

  // SAFETY:
  // The probe's recovery trap entry is installed on this sole running hart.
  // Firmware must preserve any counter access it grants for subsequent logs.
  unsafe { arch::init_time() };

  logging::info!(
    Boot,
    "kernel entered (hart={hart_id}, dtb={dtb_phys:#x}, kernel_start={kernel_phys_start:#x})"
  );

  logging::info!(Boot, "trap handling ready");

  // SAFETY:
  // Architecture bootstrap enters `main` with the complete active Sv39 root
  // page table writably identity-mapped. No other hart or code modifies the
  // paging entries reserved for bootstrap use while this capability exists.
  let mut bootstrap_paging = match unsafe { arch::BootstrapPaging::claim() } {
    Ok(paging) => paging,
    Err(error) => {
      logging::error!(
        Paging,
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
        Paging,
        "Failed to establish higher-half DTB mapping ({error:?}); \
         kernel startup is unrecoverable, halting"
      );

      arch::halt();
    }
  };

  let dtb_ptr = core::ptr::with_exposed_provenance::<u8>(dtb_virtual.as_usize());

  // SAFETY:
  // `BootstrapPaging::map_fdt` established a readable mapping containing the
  // complete DTB. The `into_kernel` transition preserves all pages backing this
  // blob at the same virtual alias. That mapping remains readable and unchanged
  // for the kernel's lifetime, and its frames are excluded from allocation.
  let fdt: Fdt<'static> = match unsafe { Fdt::from_ptr(dtb_ptr) } {
    Ok(fdt) => fdt,
    Err(error) => {
      logging::error!(
        Boot,
        "Failed to parse FDT ({error:?}); kernel startup is unrecoverable, halting"
      );

      arch::halt();
    }
  };

  #[cfg(all(feature = "log-trace", feature = "log-boot"))]
  log_fdt(&fdt);

  let Some(timebase_hz) = timer_frequency(&fdt) else {
    logging::error!(Timer, "Missing or invalid CPU timebase frequency, halting");
    arch::halt();
  };

  let Some(kernel_range) = memory::kernel_range(kernel_phys_start) else {
    logging::error!(
      Frames,
      "Invalid physical kernel range; kernel startup is unrecoverable, halting"
    );

    arch::halt();
  };

  let kernel_sections = kernel_sections();

  logging::trace!(Paging, "Kernel sections:\n{:#?}", kernel_sections);

  let Some(dtb_range) = PhysRange::from_start_size(dtb_phys, fdt.total_size()) else {
    logging::error!(
      Frames,
      "Failed to establish DTB physical range; kernel startup is unrecoverable, halting"
    );

    arch::halt();
  };

  // SAFETY:
  // The boot environment loads the kernel at its linker-defined physical
  // address, so `kernel_range` covers its complete linker-defined boot
  // footprint. `dtb_range` covers the same firmware-provided DTB backing `fdt`.
  // The FDT is trusted to describe all other physical memory that is
  // unavailable for allocation through its reservation information.
  let mut frames = match unsafe { BootFrameAllocator::claim(fdt, kernel_range, dtb_range) } {
    Ok(frames) => frames,
    Err(error) => {
      logging::error!(
        Frames,
        "Failed to claim physical frame allocator ({error:?}); \
        kernel startup is unrecoverable, halting"
      );

      arch::halt();
    }
  };

  // SAFETY:
  // These sections come directly from the live kernel's linker symbols, and
  // the linker provides the physical load address. They cover the live
  // code, trap vector, constants, data, and stack. `dtb_range` and `dtb_virtual`
  // describe the complete blob retained by the frame allocator. No other
  // retained reference depends on a bootstrap-only mapping. `boot_interrupts`
  // keeps supervisor interrupts disabled throughout this transition. No other
  // hart accesses these paging structures.
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
        Paging,
        "Failed to enter kernel paging ({error:?}); \
        kernel startup is unrecoverable, halting"
      );

      arch::halt();
    }
  };

  logging::info!(Paging, "Kernel paging active");
  logging::trace!(Paging, "Kernel paging state:\n{:#?}", kernel_paging);

  init_kernel_allocator(kernel_paging, frames);

  run_kernel(timebase_hz, boot_interrupts)
}

/// Loads a user process, confirms timer delivery, and enters user execution.
fn run_kernel(timebase_hz: NonZeroU32, boot_interrupts: arch::InterruptGuard) -> ! {
  let process = match process::load_init() {
    Ok(process) => process,
    Err(error) => {
      logging::error!(
        Process,
        "Failed to load embedded user process ({error:?}), halting"
      );
      arch::halt();
    }
  };
  let period = Duration::from_millis(10);

  // SAFETY:
  // This is the sole running hart. Its trap vector handles supervisor timer
  // interrupts, the kernel stack remains mapped, and no other code programs
  // the timer. Counter access was probed here and is preserved by firmware.
  let result: Result<(), arch::TimerError> = unsafe { arch::init_timer(timebase_hz, period) };

  if let Err(error) = result {
    logging::error!(
      Timer,
      "Failed to initialize supervisor timer ({error:?}), halting"
    );
    arch::halt();
  }

  logging::info!(
    Timer,
    "Supervisor timer armed (period={} ms, timebase={} Hz)",
    period.as_millis(),
    timebase_hz.get(),
  );

  let initial_ticks = arch::timer_ticks();
  drop(boot_interrupts);

  // SAFETY:
  // The timer is the only unmasked source, and its handler is ready. Execution
  // remains on the mapped supervisor stack with no live memory-state borrow.
  unsafe { arch::enable_interrupts() };

  while arch::timer_ticks().wrapping_sub(initial_ticks) < 2 {
    core::hint::spin_loop();
  }

  logging::info!(Timer, "Supervisor timer interrupt delivery confirmed");

  // SAFETY:
  // Loading installed the user mappings in the still-active root on this sole
  // hart. Its trap vector is installed, timer delivery was confirmed, and no
  // memory-state or frame-window borrow remains live.
  unsafe { process.run() }
}

/// Reads a non-zero, 32-bit CPU timebase frequency from the device tree.
fn timer_frequency(fdt: &Fdt<'_>) -> Option<NonZeroU32> {
  let cpus = fdt.root().child(b"cpus")?;
  let property = cpus.property(b"timebase-frequency")?;
  let bytes: [u8; 4] = property.value().try_into().ok()?;

  NonZeroU32::new(u32::from_be_bytes(bytes))
}

/// Claims a heap and installs Rust allocation state, halting on failure.
fn init_kernel_allocator(mut paging: arch::KernelPaging, mut frames: BootFrameAllocator<'static>) {
  let heap = match paging.claim_heap(&mut frames) {
    Ok(heap) => heap,
    Err(error) => {
      logging::error!(
        Boot,
        "Failed to initialize kernel heap ({error:?}); halting"
      );
      arch::halt();
    }
  };

  logging::info!(Boot, "Kernel heap ready");
  logging::trace!(Boot, "Kernel heap state:\n{heap:#?}");

  // SAFETY:
  // This heap was claimed from paging, whose root remains active on the sole
  // running hart. No other hart uses allocation, and the permanently mapped DTB
  // is covered by the frame allocator's excluded physical range. Moving all
  // three capabilities leaves the global allocator as their only owner.
  let result: Result<(), memory::AllocatorInitError> =
    unsafe { memory::init_allocator(heap, paging, frames) };

  if let Err(error) = result {
    logging::error!(
      Boot,
      "Failed to initialize Rust allocator ({error:?}); halting"
    );
    arch::halt();
  }

  logging::info!(Boot, "Rust global allocator ready");
}

/// Reports the FDT and its memory ranges when verbose boot logging is enabled.
#[cfg(all(feature = "log-trace", feature = "log-boot"))]
fn log_fdt(fdt: &Fdt<'_>) {
  logging::trace!(Boot, "FDT data structure:\n{:#?}", fdt);

  for memory_range in fdt.memory_ranges() {
    logging::trace!(Boot, "FDT RAM: {:?}", memory_range);
  }

  for memory_reservation in fdt.reserved_memory_ranges() {
    logging::trace!(Boot, "FDT /reserved-memory: {:?}", memory_reservation);
  }

  for memory_reservation in fdt.memory_reservations() {
    logging::trace!(Boot, "FDT reservation: {:?}", memory_reservation);
  }
}

/// Handles unrecoverable Rust panics by halting the current hart.
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
  arch::halt()
}

/// Reports a Rust allocation failure and halts without allocating.
#[cfg_attr(not(rust_analyzer), alloc_error_handler)]
fn allocation_error(layout: Layout) -> ! {
  logging::error!(
    Boot,
    "Rust allocation failed (size={}, align={}); halting",
    layout.size(),
    layout.align(),
  );
  arch::halt()
}
