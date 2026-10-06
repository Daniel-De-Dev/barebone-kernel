//! The active kernel Sv39 address space and temporary physical-frame access.

use core::{fmt, ptr};

use super::{
  BOOT_FRAME_WINDOW_BASE, BootstrapPaging, FRAME_WINDOW_PTE_FLAGS, GIGAPAGE_SHIFT, GIGAPAGE_SIZE,
  MEGAPAGE_SHIFT, PAGE_MASK, PAGE_SHIFT, PAGE_SIZE, PTE_EXECUTE, PTE_READ, PTE_WRITE, PagingError,
  Sv39PageTable, VPN_MASK, activate_root, flush_address, page_table_entry,
};
use crate::memory::{
  AllocatedFrame, BootFrameAllocator, KernelSections, PhysAddr, PhysFrame, PhysRange, VirtAddr,
  VirtRange,
};

/// Virtual page mapping the L0 table that controls the kernel frame window.
///
/// This lies in the final 2 MiB below the kernel's 1 GiB virtual region.
const FIXMAP_TABLE_BASE: usize =
  BOOT_FRAME_WINDOW_BASE - GIGAPAGE_SIZE - (1usize << MEGAPAGE_SHIFT);

/// Virtual page reserved for temporary access to one physical frame.
///
/// This page begins unmapped. The adjacent table alias will allow its leaf
/// entry to be changed after the replacement root is activated.
const FIXMAP_FRAME_BASE: usize = FIXMAP_TABLE_BASE + PAGE_SIZE;

/// Index of the temporary frame slot within its controlling L0 table.
const FIXMAP_FRAME_INDEX: usize = (FIXMAP_FRAME_BASE >> PAGE_SHIFT) & VPN_MASK;

/// Exclusive control of the active kernel address space.
///
/// This capability is created only by consuming [`BootstrapPaging`] and
/// activating the replacement root. It cannot be cloned or copied and must
/// be used on the hart where that root was activated. The root need not have a
/// permanent virtual mapping: physical frames are accessed through the one-page
/// window controlled by `frame_slot`.
///
/// All page-table frames remain permanently reserved by the monotonic boot
/// allocator. No other code may change the frame-window entry while this
/// capability exists.
#[must_use = "retain the capability controlling the active kernel address space"]
pub(crate) struct KernelPaging {
  /// Allocation containing the active Sv39 root.
  root: AllocatedFrame,

  /// L0 table mapped at [`FIXMAP_TABLE_BASE`].
  fixmap_table: PhysFrame,

  /// Complete virtual pages covering the DTB at its existing alias.
  dtb_mapping: VirtRange,

  /// Writable PTE for the temporary frame slot, through the fixed table alias.
  frame_slot: *mut usize,
}

impl fmt::Debug for KernelPaging {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    formatter
      .debug_struct("KernelPaging")
      .field("root", &self.root.start_address())
      .field("dtb_mapping", &self.dtb_mapping)
      .field("fixmap_table", &self.fixmap_table.start_address())
      .field("fixmap_table_alias", &VirtAddr::new(FIXMAP_TABLE_BASE))
      .field("frame_slot_pointer", &self.frame_slot)
      .field("frame_slot_alias", &VirtAddr::new(FIXMAP_FRAME_BASE))
      .finish()
  }
}

impl BootstrapPaging {
  /// Consumes bootstrap paging and enters the normal kernel address space.
  ///
  /// Kernel section addresses are translated relative to
  /// `sections.virtual_start`. Every mapping uses a 4 KiB leaf.
  ///
  /// Only pages covering `dtb_physical` are retained at the existing
  /// `dtb_virtual` alias. No bootstrap or identity mappings are copied.
  /// All mappings are built before the root is switched, and the bootstrap
  /// capability cannot be reclaimed afterward.
  ///
  /// # Safety
  ///
  /// `sections` and `kernel_physical_start` must describe the live kernel's
  /// actual virtual sections and physical load address. They must cover all
  /// live kernel code, constants, data, stacks, and trap state. `dtb_physical`
  /// must cover the complete DTB backing any retained references, and
  /// `dtb_virtual` must be its existing bootstrap alias. No other borrowed
  /// memory may depend on a mapping omitted by the new root.
  ///
  /// The transition must run on the hart whose bootstrap root this capability
  /// controls. The returned capability must be used only while the replacement
  /// root remains active on that hart.
  ///
  /// Supervisor interrupts must remain disabled during the transition and
  /// temporary frame access. No other hart or trap handler may modify these
  /// paging structures or use the temporary frame window concurrently.
  ///
  /// # Errors
  ///
  /// Returns an error if a section or DTB range cannot be translated, mappings
  /// conflict, or a required page-table frame cannot be allocated or accessed.
  /// Errors occur before activation, leaving the bootstrap root active.
  /// The consumed capability is not returned, so startup must treat failure as
  /// unrecoverable. Allocated frames remain reserved because the boot allocator
  /// does not reclaim frames.
  pub(crate) unsafe fn into_kernel(
    mut self,
    kernel_physical_start: PhysAddr,
    sections: &KernelSections,
    dtb_virtual: VirtAddr,
    dtb_physical: PhysRange,
    frames: &mut BootFrameAllocator<'_, '_>,
  ) -> Result<KernelPaging, PagingError> {
    let (dtb_mapping, dtb_frame) = dtb_page_mapping(dtb_virtual, dtb_physical)?;
    let mut page_table = Sv39PageTable::new(&mut self, frames)?;

    for (section, permissions) in [
      (sections.text, PTE_READ | PTE_EXECUTE),
      (sections.rodata, PTE_READ),
      (sections.data, PTE_READ | PTE_WRITE),
      (sections.bss, PTE_READ | PTE_WRITE),
      (sections.stack, PTE_READ | PTE_WRITE),
    ] {
      let Some(range) = section else {
        continue;
      };

      let physical_start = range
        .start()
        .as_usize()
        .checked_sub(sections.virtual_start.as_usize())
        .and_then(|offset| kernel_physical_start.checked_add(offset))
        .and_then(PhysFrame::from_start)
        .ok_or(PagingError::InvalidKernelSection { range })?;

      page_table.map_range(&mut self, range, physical_start, permissions, frames)?;
    }

    page_table.map_range(&mut self, dtb_mapping, dtb_frame, PTE_READ, frames)?;

    let fixmap_table = page_table.prepare_fixmap(&mut self, frames)?;

    let kernel = KernelPaging {
      root: page_table.root,
      fixmap_table,
      dtb_mapping,
      frame_slot: ptr::with_exposed_provenance_mut::<usize>(FIXMAP_TABLE_BASE)
        .wrapping_add(FIXMAP_FRAME_INDEX),
    };

    // SAFETY:
    // The caller supplies the complete live kernel layout and retained DTB
    // range. Construction above installs those mappings and the permanent
    // control-table alias. All table frames remain reserved, this consumes the
    // bootstrap capability, and only `kernel` is returned after the switch.
    unsafe {
      activate_root(kernel.root.frame());
    }

    Ok(kernel)
  }
}

impl KernelPaging {
  /// Executes `operation` with `frame` temporarily mapped read/write.
  ///
  /// The alias maps exactly one 4 KiB frame and is non-executable and
  /// supervisor-only. The supplied address is valid only during `operation`;
  /// references created through it must not outlive the closure.
  /// Dereferencing it requires the caller to establish valid memory access
  /// and ownership for `frame`.
  ///
  /// Supervisor interrupts must already be disabled and remain disabled
  /// throughout this call.
  ///
  /// The mapping is removed when the closure returns normally. Clean up does
  /// not run during unwinding; current kernel panics halt without unwinding.
  ///
  /// # Errors
  ///
  /// Returns an error if the frame cannot be represented by Sv39 or the
  /// temporary slot is already occupied.
  pub(crate) fn with_frame<R>(
    &mut self,
    frame: PhysFrame,
    operation: impl FnOnce(VirtAddr) -> R,
  ) -> Result<R, PagingError> {
    let entry = page_table_entry(frame.start_address(), FRAME_WINDOW_PTE_FLAGS)?;

    // SAFETY:
    // `KernelPaging` guarantees that the controlling L0 table is permanently
    // mapped read/write and that `frame_slot` points to one of its entries.
    let existing = unsafe { ptr::read_volatile(self.frame_slot) };

    if existing != 0 {
      return Err(PagingError::PageAlreadyMapped {
        address: FIXMAP_FRAME_BASE,
        entry: existing,
      });
    }

    // SAFETY:
    // The entry pointer is valid and exclusively controlled by this capability,
    // and the new leaf maps one representable, aligned physical frame.
    unsafe {
      ptr::write_volatile(self.frame_slot, entry);
    }

    let address = VirtAddr::new(FIXMAP_FRAME_BASE);

    // Make the newly installed mapping available.
    flush_address(address);

    let result = operation(address);

    // SAFETY:
    // `frame_slot` remains a valid, exclusively controlled pointer into the
    // permanently mapped L0 table. Clear its temporary frame mapping.
    unsafe {
      ptr::write_volatile(self.frame_slot, 0);
    }

    flush_address(address);

    Ok(result)
  }

  /// Zeroes every byte in an allocated physical frame through the kernel's
  /// temporary frame window.
  ///
  /// # Errors
  ///
  /// Returns an error if the physical address cannot be represented by Sv39
  /// or the temporary frame slot is already occupied.
  #[expect(
    clippy::needless_pass_by_ref_mut,
    reason = "the mutable borrow intentionally provides exclusive access to \
              the allocated frame while its memory is initialized"
  )]
  pub(crate) fn zero_frame(&mut self, frame: &mut AllocatedFrame) -> Result<(), PagingError> {
    self.with_frame(frame.frame(), |address| {
      let pointer = ptr::with_exposed_provenance_mut::<u8>(address.as_usize());

      // SAFETY:
      // The window maps the complete 4 KiB frame for this closure. The mutable
      // `AllocatedFrame` borrow provides exclusive ownership of the RAM being
      // initialized, and this operation creates no persistent references.
      unsafe {
        ptr::write_bytes(pointer, 0, PAGE_SIZE);
      }
    })
  }
}

impl Sv39PageTable {
  /// Maps the frame-window L0 table into its own control page, leaving the
  /// adjacent temporary frame slot empty.
  ///
  /// # Errors
  ///
  /// Returns an error if the tables cannot be allocated or accessed, or either
  /// fixmap page is already occupied.
  fn prepare_fixmap(
    &mut self,
    bootstrap: &mut BootstrapPaging,
    frames: &mut BootFrameAllocator<'_, '_>,
  ) -> Result<PhysFrame, PagingError> {
    let vpn2 = (FIXMAP_TABLE_BASE >> GIGAPAGE_SHIFT) & VPN_MASK;
    let vpn1 = (FIXMAP_TABLE_BASE >> MEGAPAGE_SHIFT) & VPN_MASK;

    let level_1 = bootstrap.next_table(self.root.frame(), vpn2, frames)?;
    let level_0 = bootstrap.next_table(level_1, vpn1, frames)?;
    let existing = bootstrap.read_page_table_entry(level_0, FIXMAP_FRAME_INDEX)?;

    if existing != 0 {
      return Err(PagingError::PageAlreadyMapped {
        address: FIXMAP_FRAME_BASE,
        entry: existing,
      });
    }

    self.map_page(
      bootstrap,
      VirtAddr::new(FIXMAP_TABLE_BASE),
      level_0,
      PTE_READ | PTE_WRITE,
      frames,
    )?;

    Ok(level_0)
  }
}

/// Computes the aligned virtual range and first physical frame covering the
/// DTB.
///
/// # Errors
///
/// Returns an error if the physical and virtual page offsets differ or the
/// complete page coverage cannot be represented.
#[expect(
  clippy::arithmetic_side_effects,
  reason = "PhysRange guarantees strictly ordered physical bounds"
)]
fn dtb_page_mapping(
  virtual_address: VirtAddr,
  physical_range: PhysRange,
) -> Result<(VirtRange, PhysFrame), PagingError> {
  let invalid_mapping = || PagingError::InvalidDtbMapping {
    virtual_address,
    physical_range,
  };

  let physical_start = physical_range.start().as_usize();
  let virtual_start = virtual_address.as_usize();

  if virtual_start & PAGE_MASK != physical_start & PAGE_MASK {
    return Err(invalid_mapping());
  }

  let size = physical_range.end().as_usize() - physical_start;
  let virtual_end = virtual_address
    .checked_add(size)
    .and_then(|end| end.checked_add(PAGE_MASK))
    .ok_or_else(invalid_mapping)?;

  let virtual_range = VirtRange::new(
    VirtAddr::new(virtual_start & !PAGE_MASK),
    VirtAddr::new(virtual_end.as_usize() & !PAGE_MASK),
  )
  .ok_or_else(invalid_mapping)?;

  let physical_frame = PhysFrame::from_start(PhysAddr::new(physical_start & !PAGE_MASK))
    .ok_or_else(invalid_mapping)?;

  Ok((virtual_range, physical_frame))
}
