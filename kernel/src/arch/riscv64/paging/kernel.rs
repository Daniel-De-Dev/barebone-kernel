//! The active kernel Sv39 address space and persistent/temporary frame
//! mappings.

use core::{fmt, ptr};

#[cfg(all(feature = "log-debug", feature = "log-paging"))]
use super::PtePermissions;
use super::{
  BOOT_FRAME_WINDOW_BASE, BootstrapPaging, FRAME_WINDOW_PTE_FLAGS, FrameAccess, GIGAPAGE_SHIFT,
  GIGAPAGE_SIZE, MEGAPAGE_SHIFT, PAGE_MASK, PAGE_SHIFT, PAGE_SIZE, PagePrivilege, PagingError,
  SV39_HIGH_HALF_BASE, SV39_LOW_HALF_END, Sv39PageTable, VPN_MASK, activate_root, flush_address,
  flush_all, map_page, page_table_entry,
};
use crate::{
  logging,
  memory::{
    AllocatedFrame, FrameAllocator, HeapBacking, HeapError, KernelHeap, KernelSections,
    PagePermissions, PhysAddr, PhysFrame, PhysRange, VirtAddr, VirtRange,
  },
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

/// Inclusive base of the upper-half root slot designated for heap allocation.
const HEAP_START: usize = 0xffff_ffe0_0000_0000;

/// Exclusive upper bound of the heap reservation.
const HEAP_END: usize = HEAP_START + GIGAPAGE_SIZE;

/// Fixed virtual range available for a heap claim.
const HEAP_RANGE: VirtRange = VirtRange::new(VirtAddr::new(HEAP_START), VirtAddr::new(HEAP_END))
  .expect("constant heap bounds must be ordered");

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

  /// Whether the heap reservation was claimed, including failed backing attempts.
  heap_claimed: bool,
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
      .field("heap_claimed", &self.heap_claimed)
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
    frames: &mut impl FrameAllocator,
  ) -> Result<KernelPaging, PagingError> {
    let (dtb_mapping, dtb_frame) = dtb_page_mapping(dtb_virtual, dtb_physical)?;

    logging::debug!(
      Paging,
      "retaining DTB pa=[{:#x}, {:#x}) at alias_va={:#x}; page coverage va=[{:#x}, {:#x})",
      dtb_physical.start(),
      dtb_physical.end(),
      dtb_virtual.as_usize(),
      dtb_mapping.start().as_usize(),
      dtb_mapping.end().as_usize(),
    );

    let mut page_table = Sv39PageTable::new(&mut self, frames)?;

    for (section, permissions) in [
      (sections.text, PagePermissions::ReadExecute),
      (sections.rodata, PagePermissions::ReadOnly),
      (sections.data, PagePermissions::ReadWrite),
      (sections.bss, PagePermissions::ReadWrite),
      (sections.stack, PagePermissions::ReadWrite),
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

    page_table.map_range(
      &mut self,
      dtb_mapping,
      dtb_frame,
      PagePermissions::ReadOnly,
      frames,
    )?;

    let fixmap_table = page_table.prepare_fixmap(&mut self, frames)?;

    let kernel = KernelPaging {
      root: page_table.root,
      fixmap_table,
      dtb_mapping,
      frame_slot: ptr::with_exposed_provenance_mut::<usize>(FIXMAP_TABLE_BASE)
        .wrapping_add(FIXMAP_FRAME_INDEX),
      heap_claimed: false,
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
  /// Maps an owned frame at one previously unmapped lower-half user page.
  ///
  /// The null page is excluded. Missing tables are allocated and zeroed, and
  /// local translations are synchronized even after partial failure. Existing
  /// leaves are never replaced. Data and table frames remain allocated.
  /// Use only on the owning hart with this root active and supervisor interrupts
  /// disabled. No concurrent access to the tables or frame window is permitted.
  ///
  /// # Errors
  ///
  /// Returns an error for an invalid or unaligned user address, an occupied
  /// page, malformed tables, or unavailable physical frames or frame access.
  pub(crate) fn map_user_page(
    &mut self,
    virtual_address: VirtAddr,
    frame: AllocatedFrame,
    permissions: PagePermissions,
    frames: &mut impl FrameAllocator,
  ) -> Result<(), PagingError> {
    let address = virtual_address.as_usize();
    if !(PAGE_SIZE..SV39_LOW_HALF_END).contains(&address) {
      return Err(PagingError::VirtualAddressNotUser { address });
    }

    let result = map_page(
      self.root.frame(),
      self,
      virtual_address,
      frame.into_frame(),
      permissions,
      PagePrivilege::User,
      frames,
    );
    flush_all();
    result
  }

  /// Claims the fixed heap reservation and backs its highest page.
  ///
  /// The root slot must be empty. Once claimed, ordinary `map_page` calls cannot
  /// use this range, and dropping the heap does not permit a second claim.
  /// Use only on the owning hart with this root active and supervisor
  /// interrupts disabled. No other hart or trap handler may access these
  /// tables or the temporary window concurrently.
  ///
  /// # Errors
  ///
  /// Returns an error if the heap was already claimed, its root slot is in use,
  /// or its first page cannot be backed. Backing failure leaves the reservation
  /// claimed and retains any allocated frames and installed table entries.
  pub(crate) fn claim_heap(
    &mut self,
    frames: &mut impl FrameAllocator,
  ) -> Result<KernelHeap, HeapError<PagingError>> {
    if self.heap_claimed {
      return Err(HeapError::Backing(PagingError::KernelHeapAlreadyClaimed));
    }

    let index = (HEAP_START >> GIGAPAGE_SHIFT) & VPN_MASK;
    let entry = self
      .read_page_table_entry(self.root.frame(), index)
      .map_err(HeapError::Backing)?;

    if entry != 0 {
      return Err(HeapError::Backing(PagingError::RootEntryInUse {
        index,
        entry,
      }));
    }

    self.heap_claimed = true;
    let mut backing = self.heap_backing(frames);

    // SAFETY:
    // The empty root slot covers the entire fixed heap range. The one-shot
    // claim and ordinary mapper's reservation check prevent another allocator
    // or unrelated mapping from using it. The backing retains every installed
    // heap mapping and all physical frames are permanently reserved.
    unsafe { KernelHeap::new(HEAP_RANGE, &mut backing) }
  }

  /// Borrows paging and frame allocation to back this hart's claimed heap.
  ///
  /// The adapter borrows both arguments for its lifetime. Use it only for pages
  /// requested by the heap claimed from this address space, on the owning hart
  /// with this root active and supervisor interrupts disabled. No other hart or
  /// trap handler may access the tables or temporary window concurrently.
  pub(crate) fn heap_backing<F: FrameAllocator>(
    &mut self,
    frames: &mut F,
  ) -> impl HeapBacking<Error = PagingError> {
    KernelHeapBacking {
      paging: self,
      frames,
    }
  }

  /// Maps one previously unmapped upper-half virtual page to an owned frame.
  ///
  /// The 4 KiB mapping is supervisor-only and remains installed after this
  /// call. Missing intermediate tables are allocated and zeroed through the
  /// normal kernel frame window. The frame-window page itself is reserved.
  /// A claimed heap reservation is also excluded from this general mapper.
  ///
  /// Use only on the owning hart with this root active and supervisor
  /// interrupts disabled. No other hart or trap handler may access these
  /// tables or the temporary window concurrently.
  ///
  /// # Errors
  ///
  /// Returns an error for an invalid, unaligned, reserved, or already mapped
  /// virtual page, an unrepresentable physical frame, frame exhaustion, or
  /// malformed intermediate entries. Local translations are synchronized
  /// even after partial failure. Remote invalidation is not supported.
  pub(crate) fn map_page(
    &mut self,
    virtual_address: VirtAddr,
    frame: AllocatedFrame,
    permissions: PagePermissions,
    frames: &mut impl FrameAllocator,
  ) -> Result<(), PagingError> {
    let address = virtual_address.as_usize();

    if self.heap_claimed && (HEAP_START..HEAP_END).contains(&address) {
      return Err(PagingError::ReservedVirtualPage { address });
    }

    self.install_page(virtual_address, frame, permissions, frames)
  }

  /// Validates and installs one supervisor-only upper-half page mapping.
  ///
  /// The caller must enforce virtual reservation ownership. Page-table changes
  /// are followed by local translation synchronization, including partial failure.
  ///
  /// # Errors
  ///
  /// Returns an error for invalid addresses, conflicts, or failed table access
  /// or allocation.
  fn install_page(
    &mut self,
    virtual_address: VirtAddr,
    frame: AllocatedFrame,
    permissions: PagePermissions,
    frames: &mut impl FrameAllocator,
  ) -> Result<(), PagingError> {
    let address = virtual_address.as_usize();

    if address < SV39_HIGH_HALF_BASE {
      return Err(PagingError::VirtualAddressNotKernel { address });
    }

    if address == FIXMAP_FRAME_BASE {
      return Err(PagingError::ReservedVirtualPage { address });
    }

    let root = self.root.frame();
    let result = map_page(
      root,
      self,
      virtual_address,
      frame.into_frame(),
      permissions,
      PagePrivilege::Supervisor,
      frames,
    );

    flush_all();

    result
  }

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

    logging::trace!(
      Paging,
      "mapped kernel frame window va={FIXMAP_FRAME_BASE:#x} -> pa={:#x} \
       size={PAGE_SIZE} perms={} pte={entry:#x}",
      frame.start_address(),
      PtePermissions(FRAME_WINDOW_PTE_FLAGS),
    );

    let result = operation(address);

    // SAFETY:
    // `frame_slot` remains a valid, exclusively controlled pointer into the
    // permanently mapped L0 table. Clear its temporary frame mapping.
    unsafe {
      ptr::write_volatile(self.frame_slot, 0);
    }

    flush_address(address);

    logging::trace!(
      Paging,
      "unmapped kernel frame window va={FIXMAP_FRAME_BASE:#x}",
    );

    Ok(result)
  }

  /// Zeroes every byte in an allocated physical frame through the kernel's
  /// temporary frame window.
  ///
  /// # Errors
  ///
  /// Returns an error if the physical address cannot be represented by Sv39
  /// or the temporary frame slot is already occupied.
  pub(crate) fn zero_frame(&mut self, frame: &mut AllocatedFrame) -> Result<(), PagingError> {
    FrameAccess::zero_frame(self, frame)
  }
}

/// Backs heap pages using an active kernel root and a frame allocator.
struct KernelHeapBacking<'paging, 'frames, F> {
  /// Exclusive control of the active kernel root.
  paging: &'paging mut KernelPaging,

  /// Source of permanently reserved data and page-table frames.
  frames: &'frames mut F,
}

// SAFETY:
// Each successful request maps a fresh, exclusively allocated RAM frame at one
// page of the claimed heap. The frame is zeroed first and the leaf is RW, NX,
// and supervisor-only. Neither data nor table frames are reclaimed. Earlier
// mappings survive later errors. install_page never replaces an existing leaf
// and cannot return an error after installing the requested heap leaf.
unsafe impl<F: FrameAllocator> HeapBacking for KernelHeapBacking<'_, '_, F> {
  type Error = PagingError;

  fn map_page(&mut self, address: VirtAddr) -> Result<(), Self::Error> {
    let value = address.as_usize();

    if !self.paging.heap_claimed || !(HEAP_START..HEAP_END).contains(&value) {
      return Err(PagingError::ReservedVirtualPage { address: value });
    }

    if value & PAGE_MASK != 0 {
      return Err(PagingError::VirtualAddressNotPageAligned { address: value });
    }

    let mut frame = self
      .frames
      .allocate()
      .ok_or(PagingError::OutOfPhysicalFrames)?;
    self.paging.zero_frame(&mut frame)?;
    self
      .paging
      .install_page(address, frame, PagePermissions::ReadWrite, self.frames)
  }
}

// SAFETY:
// The kernel capability exclusively controls its one-page frame window. Its
// inherent with_frame maps the supplied frame read/write and removes the alias
// before returning normally, subject to the capability's single-hart contract.
unsafe impl FrameAccess for KernelPaging {
  fn with_frame<R>(
    &mut self,
    frame: PhysFrame,
    operation: impl FnOnce(VirtAddr) -> R,
  ) -> Result<R, PagingError> {
    Self::with_frame(self, frame, operation)
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
  #[expect(
    clippy::needless_pass_by_ref_mut,
    reason = "the mutable borrow keeps the root exclusively controlled while its tables change"
  )]
  fn prepare_fixmap(
    &mut self,
    bootstrap: &mut BootstrapPaging,
    frames: &mut impl FrameAllocator,
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

    map_page(
      self.root.frame(),
      bootstrap,
      VirtAddr::new(FIXMAP_TABLE_BASE),
      level_0,
      PagePermissions::ReadWrite,
      PagePrivilege::Supervisor,
      frames,
    )?;

    logging::debug!(
      Paging,
      "prepared kernel frame window: table_pa={:#x} control_va={FIXMAP_TABLE_BASE:#x} \
       frame_va={FIXMAP_FRAME_BASE:#x} (unmapped)",
      level_0.start_address(),
    );

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
