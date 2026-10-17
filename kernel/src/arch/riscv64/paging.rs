//! RISC-V Sv39 paging support.
//!
//! This module provides bootstrap paging, construction of the kernel address
//! space, persistent runtime mappings, and temporary access to physical frames.
//!
//! With log-paging enabled, Debug summarizes mapping ranges and root changes;
//! Trace records installed leaves, initialized tables, new table links, and
//! kernel frame-window changes. PTE reads and bootstrap window accesses used
//! for table traversal are not logged individually.
// TODO: Support larger leaves for aligned, physically contiguous ranges
//       with uniform permissions.
// TODO: Protect the frame window against interrupt re-entry before handlers
//       begin using paging.
// TODO: Add synchronization and remote translation invalidation before
//       multiple kernel harts share mutable mappings.
// TODO: Add automatic temporary-mapping cleanup if recoverable unwinding
//       or early returns are introduced after installing a mapping.

mod access;
mod kernel;

use access::FrameAccess;

pub(crate) use kernel::KernelPaging;

use core::{
  arch::asm,
  ptr,
  sync::atomic::{AtomicBool, Ordering},
};

use crate::{
  logging,
  memory::{
    AllocatedFrame, FrameAllocator, PagePermissions, PhysAddr, PhysFrame, PhysRange, VirtAddr,
    VirtRange,
  },
};

#[cfg(all(feature = "log-debug", feature = "log-paging"))]
use core::fmt;

/// Sign bit of an Sv39 virtual address.
const SV39_SIGN_BIT: usize = 38;

/// Lowest canonical virtual address in the upper half of Sv39.
const SV39_HIGH_HALF_BASE: usize = usize::MAX << SV39_SIGN_BIT;

/// First root-table index belonging to the canonical Sv39 upper half.
const SV39_HIGH_HALF_VPN2: usize = (SV39_HIGH_HALF_BASE >> GIGAPAGE_SHIFT) & VPN_MASK;

/// First virtual address outside the canonical Sv39 lower half.
const SV39_LOW_HALF_END: usize = 1usize << SV39_SIGN_BIT;

/// Number of low address bits forming a page offset.
const PAGE_SHIFT: usize = 12;

/// Size in bytes of one Sv39 page.
const PAGE_SIZE: usize = 1 << PAGE_SHIFT;

/// Mask selecting an offset within one Sv39 page.
const PAGE_MASK: usize = PAGE_SIZE - 1;

/// Bit offset of an Sv39 level-1 virtual page number.
const MEGAPAGE_SHIFT: usize = 21;

/// Number of entries in one Sv39 page table.
const PAGE_TABLE_ENTRIES: usize = 512;

/// Bit offset of an Sv39 level-2 virtual page number.
const GIGAPAGE_SHIFT: usize = 30;

/// Size in bytes mapped by an Sv39 level-2 leaf.
const GIGAPAGE_SIZE: usize = 1 << GIGAPAGE_SHIFT;

/// Mask selecting an offset within a level-2 leaf mapping.
const GIGAPAGE_MASK: usize = GIGAPAGE_SIZE - 1;

/// Mask selecting one Sv39 virtual page-number field.
const VPN_MASK: usize = 0x1ff;

/// Bit offset of the physical page number within an Sv39 PTE.
const PTE_PPN_SHIFT: usize = 10;

/// PTE bit marking an entry as valid.
const PTE_VALID: usize = 1 << 0;

/// PTE bit permitting reads through a leaf mapping.
const PTE_READ: usize = 1 << 1;

/// PTE bit permitting writes through a leaf mapping.
const PTE_WRITE: usize = 1 << 2;

/// PTE bit permitting instruction fetches through a leaf mapping.
const PTE_EXECUTE: usize = 1 << 3;

/// PTE bit recording that a leaf mapping has been accessed.
const PTE_ACCESSED: usize = 1 << 6;

/// PTE bit recording that a writable leaf mapping has been modified.
const PTE_DIRTY: usize = 1 << 7;

/// Bits distinguishing an Sv39 leaf from a non-leaf entry.
const PTE_LEAF_MASK: usize = PTE_READ | PTE_WRITE | PTE_EXECUTE;

/// Formats PTE read/write/execute permissions for logging without allocation.
#[cfg(all(feature = "log-debug", feature = "log-paging"))]
struct PtePermissions(usize);

#[cfg(all(feature = "log-debug", feature = "log-paging"))]
impl fmt::Display for PtePermissions {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(
      formatter,
      "{}{}{}",
      if self.0 & PTE_READ != 0 { 'r' } else { '-' },
      if self.0 & PTE_WRITE != 0 { 'w' } else { '-' },
      if self.0 & PTE_EXECUTE != 0 { 'x' } else { '-' },
    )
  }
}

/// Flags for a read-only boot-time FDT leaf mapping.
const BOOT_FDT_PTE_FLAGS: usize = PTE_VALID | PTE_READ | PTE_ACCESSED;

/// First virtual address of the boot-time FDT mapping window.
const BOOT_FDT_WINDOW_BASE: usize = SV39_HIGH_HALF_BASE;

/// Root-table index containing the start of the boot-time FDT window.
const BOOT_FDT_WINDOW_VPN2: usize = SV39_HIGH_HALF_VPN2;

/// Number of consecutive level-2 leaves available to the boot-time FDT window.
///
/// Five leaves cover the largest `u32`-sized FDT even when it begins at the
/// end of its first 1 GiB region.
const BOOT_FDT_WINDOW_GIGAPAGES: usize = 5;

/// Root-table index reserved for temporary access to physical frames.
///
/// The final root entry covers the highest 1 GiB of canonical Sv39 addresses.
const BOOT_FRAME_WINDOW_VPN2: usize = VPN_MASK;

/// First virtual address of the temporary physical-frame access window.
const BOOT_FRAME_WINDOW_BASE: usize =
  SV39_HIGH_HALF_BASE + ((BOOT_FRAME_WINDOW_VPN2 - SV39_HIGH_HALF_VPN2) * GIGAPAGE_SIZE);

/// Flags for read/write, non-executable supervisor frame-window leaves.
const FRAME_WINDOW_PTE_FLAGS: usize = PTE_VALID | PTE_READ | PTE_WRITE | PTE_ACCESSED | PTE_DIRTY;

/// Bit position of `satp.MODE` on RV64.
const SATP_MODE_SHIFT: usize = 60;

/// Mask selecting `satp.MODE` after shifting it to bit zero.
const SATP_MODE_MASK: usize = 0xf;

/// `satp.MODE` value selecting Sv39.
const SATP_MODE_SV39: usize = 8;

/// Number of physical page-number bits represented by Sv39.
const PHYSICAL_PAGE_NUMBER_BITS: usize = 44;

/// Mask selecting `satp.PPN`.
const SATP_PPN_MASK: usize = (1usize << PHYSICAL_PAGE_NUMBER_BITS) - 1;

/// Number of physical-address bits representable by Sv39.
const PHYSICAL_ADDRESS_BITS: usize = PHYSICAL_PAGE_NUMBER_BITS + PAGE_SHIFT;

/// Largest physical byte address representable by Sv39.
const MAX_PHYSICAL_ADDRESS: usize = (1usize << PHYSICAL_ADDRESS_BITS) - 1;

/// One 4 KiB Sv39 page table containing 512 64-bit entries.
#[repr(C, align(4096))]
struct PageTable {
  /// Raw Sv39 page-table entries.
  entries: [usize; PAGE_TABLE_ENTRIES],
}

/// Exclusive access to the inherited Sv39 bootstrap address space.
///
/// Construction establishes that `root_virtual` maps the complete active Sv39
/// root page table read/write and that this value exclusively controls the
/// bootstrap paging entries reserved by this module.
///
/// The capability is valid only while that bootstrap address space remains
/// active.
pub(crate) struct BootstrapPaging {
  /// Identity-mapped virtual address of the active bootstrap root table.
  root_virtual: VirtAddr,
}

/// Tracks whether the bootstrap paging capability has already been claimed.
///
/// Once set, this flag is never cleared, preventing construction of more than
/// one [`BootstrapPaging`] capability during the lifetime of the kernel.
static BOOTSTRAP_PAGING_CLAIMED: AtomicBool = AtomicBool::new(false);

impl BootstrapPaging {
  /// Claims the bootstrap paging capability for the active Sv39 address space.
  ///
  /// This capability can be claimed only once. A successful claim remains
  /// permanent even if the returned [`BootstrapPaging`] value is later dropped.
  ///
  /// # Safety
  ///
  /// The active Sv39 root page table must be completely and writably
  /// identity-mapped. The paging entries reserved by this module must not be
  /// modified by any other code or hart while this capability exists, and the
  /// active root must not be replaced except by consuming this capability.
  ///
  /// # Errors
  ///
  /// Returns [`PagingError::UnexpectedAddressTranslationMode`] if the active
  /// address-translation mode is not Sv39, or
  /// [`PagingError::BootstrapPagingAlreadyClaimed`] if the bootstrap paging
  /// capability has already been claimed.
  pub(crate) unsafe fn claim() -> Result<Self, PagingError> {
    let root = active_root_table()?;

    if BOOTSTRAP_PAGING_CLAIMED
      .compare_exchange(false, true, Ordering::Relaxed, Ordering::Relaxed)
      .is_err()
    {
      return Err(PagingError::BootstrapPagingAlreadyClaimed);
    }

    logging::debug!(Paging, "claimed bootstrap Sv39 root pa={root:#x}");

    Ok(Self {
      root_virtual: VirtAddr::new(root.as_usize()),
    })
  }

  /// Maps the physical region containing `dtb` into a higher-half boot window.
  ///
  /// The mapping preserves `dtb`'s offset within its first 1 GiB physical region,
  /// so the returned virtual address refers to the same byte as `dtb`. The window
  /// consists of consecutive read-only, non-executable Sv39 level-2 leaves.
  ///
  /// # Errors
  ///
  /// Returns [`PagingError::RootEntryInUse`] if the required virtual window is
  /// occupied, or [`PagingError::PhysicalAddressNotRepresentable`] if the DTB
  /// window requires a physical address that cannot be represented by Sv39.
  #[expect(
    clippy::arithmetic_side_effects,
    reason = "window indices are statically bounded and the DTB address is \
              validated before physical-address arithmetic"
  )]
  pub(crate) fn map_fdt(&mut self, dtb: PhysAddr) -> Result<VirtAddr, PagingError> {
    if dtb.as_usize() > MAX_PHYSICAL_ADDRESS {
      return Err(PagingError::PhysicalAddressNotRepresentable { address: dtb });
    }

    let physical_base = PhysAddr::new(dtb.as_usize() & !GIGAPAGE_MASK);

    /*
     * Validate the complete window before modifying the active page table so an
     * error cannot leave a partially installed mapping.
     */
    for window_index in 0..BOOT_FDT_WINDOW_GIGAPAGES {
      let root_index = BOOT_FDT_WINDOW_VPN2 + window_index;

      // SAFETY:
      // `BootstrapPaging` guarantees that `root_virtual` maps the complete readable
      // active bootstrap root table while this capability exists.
      let existing = unsafe { read_mapped_page_table_entry(self.root_virtual, root_index)? };

      if existing != 0 {
        return Err(PagingError::RootEntryInUse {
          index: root_index,
          entry: existing,
        });
      }

      let physical_offset = window_index * GIGAPAGE_SIZE;

      let physical_address = PhysAddr::new(physical_base.as_usize() + physical_offset);

      if physical_address.as_usize() > MAX_PHYSICAL_ADDRESS {
        return Err(PagingError::PhysicalAddressNotRepresentable {
          address: physical_address,
        });
      }
    }

    /*
     * Every required entry and physical address has been validated. Install the
     * read-only level-2 leaves.
     */
    for window_index in 0..BOOT_FDT_WINDOW_GIGAPAGES {
      let root_index = BOOT_FDT_WINDOW_VPN2 + window_index;

      let physical_offset = window_index * GIGAPAGE_SIZE;

      let physical_address = PhysAddr::new(physical_base.as_usize() + physical_offset);

      let entry = page_table_entry(physical_address, BOOT_FDT_PTE_FLAGS)?;

      // SAFETY:
      // `BootstrapPaging` guarantees that `root_virtual` maps the complete writable
      // active bootstrap root table. This entry was verified to be unused before any
      // FDT mappings were installed.
      unsafe {
        write_mapped_page_table_entry(self.root_virtual, root_index, entry)?;
      }

      logging::trace!(
        Paging,
        "installed bootstrap DTB leaf va={:#x} -> pa={physical_address:#x} \
         size={GIGAPAGE_SIZE} perms={} root_index={root_index} pte={entry:#x}",
        BOOT_FDT_WINDOW_BASE + physical_offset,
        PtePermissions(BOOT_FDT_PTE_FLAGS),
      );
    }

    flush_all();

    let dtb_offset = dtb.as_usize() & GIGAPAGE_MASK;

    let virtual_address = BOOT_FDT_WINDOW_BASE + dtb_offset;

    logging::debug!(
      Paging,
      "bootstrap DTB window ready: va_base={BOOT_FDT_WINDOW_BASE:#x} \
       pa_base={physical_base:#x} leaves={BOOT_FDT_WINDOW_GIGAPAGES} \
       leaf_size={GIGAPAGE_SIZE}; DTB pa={dtb:#x} alias_va={virtual_address:#x}",
    );

    Ok(VirtAddr::new(virtual_address))
  }

  /// Executes `operation` while `frame` is temporarily accessible through the
  /// bootstrap physical-frame window.
  ///
  /// The temporary mapping uses a single Sv39 level-2 leaf. The physical 1 GiB
  /// region containing `frame` is mapped at [`BOOT_FRAME_WINDOW_BASE`], and the
  /// returned virtual address preserves the frame's offset within that region.
  ///
  /// The mapping exists only while `operation` executes and is removed before
  /// this function returns.
  ///
  /// # Errors
  ///
  /// Returns [`PagingError::RootEntryInUse`] if the reserved frame-window entry
  /// is already occupied, or
  /// [`PagingError::PhysicalAddressNotRepresentable`] if `frame` cannot be
  /// represented by Sv39.
  #[expect(
    clippy::arithmetic_side_effects,
    reason = "the root index and frame offset are bounded by the Sv39 page-table and gigapage sizes"
  )]
  fn with_frame<R>(
    &mut self,
    frame: PhysFrame,
    operation: impl FnOnce(VirtAddr) -> R,
  ) -> Result<R, PagingError> {
    // SAFETY:
    // `BootstrapPaging` guarantees that `root_virtual` maps the complete
    // readable active bootstrap root table.
    let existing =
      unsafe { read_mapped_page_table_entry(self.root_virtual, BOOT_FRAME_WINDOW_VPN2)? };

    if existing != 0 {
      return Err(PagingError::RootEntryInUse {
        index: BOOT_FRAME_WINDOW_VPN2,
        entry: existing,
      });
    }

    let frame_address = frame.start_address();

    if frame_address.as_usize() > MAX_PHYSICAL_ADDRESS {
      return Err(PagingError::PhysicalAddressNotRepresentable {
        address: frame_address,
      });
    }

    let physical_base = PhysAddr::new(frame_address.as_usize() & !GIGAPAGE_MASK);

    let entry = page_table_entry(physical_base, FRAME_WINDOW_PTE_FLAGS)?;

    // SAFETY:
    // `BootstrapPaging` guarantees that `root_virtual` maps the complete
    // writable active bootstrap root table.
    unsafe {
      write_mapped_page_table_entry(self.root_virtual, BOOT_FRAME_WINDOW_VPN2, entry)?;
    }

    let frame_offset = frame_address.as_usize() & GIGAPAGE_MASK;

    let virtual_address = VirtAddr::new(BOOT_FRAME_WINDOW_BASE + frame_offset);

    flush_address(virtual_address);

    let result = operation(virtual_address);

    // SAFETY:
    // `BootstrapPaging` guarantees that `root_virtual` maps the complete
    // writable active bootstrap root table.
    unsafe {
      write_mapped_page_table_entry(self.root_virtual, BOOT_FRAME_WINDOW_VPN2, 0)?;
    }

    flush_address(virtual_address);

    Ok(result)
  }
}

// SAFETY:
// The bootstrap capability exclusively controls its temporary window. Its
// inherent with_frame maps the complete supplied frame read/write and removes
// the alias before returning normally.
unsafe impl FrameAccess for BootstrapPaging {
  fn with_frame<R>(
    &mut self,
    frame: PhysFrame,
    operation: impl FnOnce(VirtAddr) -> R,
  ) -> Result<R, PagingError> {
    Self::with_frame(self, frame, operation)
  }
}

/// An Sv39 address space being constructed by the kernel.
struct Sv39PageTable {
  /// Allocation containing the level-2 root table.
  root: AllocatedFrame,
}

impl Sv39PageTable {
  /// Allocates a new empty Sv39 address space.
  ///
  /// # Errors
  ///
  /// Returns an error if no root frame can be allocated or initialized.
  fn new(
    access: &mut impl FrameAccess,
    frames: &mut impl FrameAllocator,
  ) -> Result<Self, PagingError> {
    let root = access.allocate_page_table(frames)?;

    logging::debug!(
      Paging,
      "created empty Sv39 root pa={:#x}",
      root.start_address(),
    );

    Ok(Self { root })
  }

  /// Maps every 4 KiB page covering `virtual_range` to consecutive physical
  /// frames beginning at `physical_start`.
  ///
  /// The virtual start must be page-aligned. The exclusive end may be
  /// unaligned, its final partial page is mapped in full.
  ///
  /// # Errors
  ///
  /// Returns an error if the virtual start is unaligned, the range includes
  /// non-canonical Sv39 addresses, the physical span is not representable, or
  /// an individual page cannot be mapped.
  ///
  /// Address spans are validated before any entries are installed. Allocation
  /// or mapping conflicts can still fail partway through; mappings already
  /// installed by this call remain in the address space.
  #[expect(
    clippy::arithmetic_side_effects,
    reason = "VirtRange bounds are ordered, and page offsets fit within the \
              validated virtual and physical spans"
  )]
  #[expect(
    clippy::needless_pass_by_ref_mut,
    reason = "the mutable borrow keeps the root exclusively controlled while its tables change"
  )]
  fn map_range(
    &mut self,
    access: &mut impl FrameAccess,
    virtual_range: VirtRange,
    physical_start: PhysFrame,
    permissions: PagePermissions,
    frames: &mut impl FrameAllocator,
  ) -> Result<(), PagingError> {
    let start = virtual_range.start().as_usize();
    let end = virtual_range.end().as_usize();

    if start & PAGE_MASK != 0 {
      return Err(PagingError::VirtualAddressNotPageAligned { address: start });
    }

    if (SV39_LOW_HALF_END..SV39_HIGH_HALF_BASE).contains(&start) {
      return Err(PagingError::VirtualAddressNotCanonical { address: start });
    }

    if start < SV39_LOW_HALF_END && end > SV39_LOW_HALF_END {
      return Err(PagingError::VirtualAddressNotCanonical {
        address: SV39_LOW_HALF_END,
      });
    }

    let size = end - start;
    let page_count = size.div_ceil(PAGE_SIZE);
    let last_offset = (page_count - 1) * PAGE_SIZE;
    let physical_base = physical_start.start_address();

    physical_base
      .checked_add(last_offset)
      .filter(|address| address.as_usize() <= MAX_PHYSICAL_ADDRESS)
      .ok_or(PagingError::PhysicalRangeNotRepresentable {
        start: physical_base,
        size,
      })?;

    logging::debug!(
      Paging,
      "mapping range va=[{start:#x}, {end:#x}) -> pa={physical_base:#x} \
       pages={page_count} page_size={PAGE_SIZE} perms={} (supervisor)",
      PtePermissions(permission_bits(permissions)),
    );

    for page_index in 0..page_count {
      let offset = page_index * PAGE_SIZE;
      let virtual_address = VirtAddr::new(start + offset);
      let physical_address = PhysAddr::new(physical_base.as_usize() + offset);

      let Some(frame) = PhysFrame::from_start(physical_address) else {
        return Err(PagingError::PhysicalRangeNotRepresentable {
          start: physical_base,
          size,
        });
      };

      map_page(
        self.root.frame(),
        access,
        virtual_address,
        frame,
        permissions,
        frames,
      )?;
    }

    Ok(())
  }
}

/// Maps one 4 KiB virtual page to `physical_frame`.
///
/// `permissions` describes supervisor-only access through the leaf.
/// Valid/accessed state is added automatically, as is the dirty bit for
/// writable mappings.
///
/// # Errors
///
/// Returns an error if the virtual address is invalid or unaligned, physical
/// frames required for intermediate tables cannot be allocated, an existing
/// intermediate entry is malformed, or the virtual page is already mapped.
fn map_page(
  root: PhysFrame,
  access: &mut impl FrameAccess,
  virtual_address: VirtAddr,
  physical_frame: PhysFrame,
  permissions: PagePermissions,
  frames: &mut impl FrameAllocator,
) -> Result<(), PagingError> {
  let address = virtual_address.as_usize();

  if address & PAGE_MASK != 0 {
    return Err(PagingError::VirtualAddressNotPageAligned { address });
  }

  if (SV39_LOW_HALF_END..SV39_HIGH_HALF_BASE).contains(&address) {
    return Err(PagingError::VirtualAddressNotCanonical { address });
  }

  let permissions = permission_bits(permissions);

  let mut flags = PTE_VALID | PTE_ACCESSED | permissions;

  if permissions & PTE_WRITE != 0 {
    flags |= PTE_DIRTY;
  }

  let entry = page_table_entry(physical_frame.start_address(), flags)?;

  let vpn2 = (address >> GIGAPAGE_SHIFT) & VPN_MASK;
  let vpn1 = (address >> MEGAPAGE_SHIFT) & VPN_MASK;
  let vpn0 = (address >> PAGE_SHIFT) & VPN_MASK;

  let level_1 = access.next_table(root, vpn2, frames)?;
  let level_0 = access.next_table(level_1, vpn1, frames)?;

  let existing = access.read_page_table_entry(level_0, vpn0)?;

  if existing != 0 {
    return Err(PagingError::PageAlreadyMapped {
      address,
      entry: existing,
    });
  }

  access.write_page_table_entry(level_0, vpn0, entry)?;

  logging::trace!(
    Paging,
    "installed leaf va={address:#x} -> pa={:#x} size={PAGE_SIZE} perms={} \
     table_pa={:#x} index={vpn0} pte={entry:#x}",
    physical_frame.start_address(),
    PtePermissions(flags),
    level_0.start_address(),
  );

  Ok(())
}

/// Encodes the supported supervisor-only permissions as Sv39 leaf bits.
const fn permission_bits(permissions: PagePermissions) -> usize {
  match permissions {
    PagePermissions::ReadOnly => PTE_READ,
    PagePermissions::ReadWrite => PTE_READ | PTE_WRITE,
    PagePermissions::ReadExecute => PTE_READ | PTE_EXECUTE,
  }
}

/// Errors produced while inspecting or modifying Sv39 paging state.
#[derive(Debug)]
pub(crate) enum PagingError {
  /// A root-table entry required for a new mapping was not empty.
  RootEntryInUse {
    /// Index of the occupied root-table entry.
    index: usize,

    /// Raw value stored in the occupied page-table entry.
    entry: usize,
  },

  /// The active `satp` translation mode was not Sv39.
  UnexpectedAddressTranslationMode {
    /// Raw value of the `satp.MODE` field.
    mode: usize,
  },

  /// A physical address cannot be represented by an Sv39 page-table entry.
  PhysicalAddressNotRepresentable {
    /// Physical address outside the Sv39 representable range.
    address: PhysAddr,
  },

  /// The physical frames covering a requested byte range cannot all be
  /// represented by Sv39, or computing the final frame address would overflow.
  PhysicalRangeNotRepresentable {
    /// Physical start address corresponding to the virtual range start.
    start: PhysAddr,

    /// Requested range size in bytes before rounding up to complete pages.
    size: usize,
  },

  /// A kernel section could not be translated into an aligned physical frame.
  InvalidKernelSection {
    /// Linker-defined virtual section range.
    range: VirtRange,
  },

  /// The DTB alias had a different page offset or its page coverage overflowed.
  InvalidDtbMapping {
    /// Virtual address of the first byte of the DTB.
    virtual_address: VirtAddr,

    /// Physical range covering the DTB bytes.
    physical_range: PhysRange,
  },

  /// No physical frame remained for a requested allocation or page table.
  OutOfPhysicalFrames,

  /// A virtual address supplied for a 4 KiB mapping was not page-aligned.
  VirtualAddressNotPageAligned {
    /// Unaligned virtual address.
    address: usize,
  },

  /// A virtual address is not canonical under Sv39.
  VirtualAddressNotCanonical {
    /// Non-canonical virtual address.
    address: usize,
  },

  /// A page-table entry expected to point to the next level was malformed or
  /// was already a leaf mapping.
  InvalidIntermediateEntry {
    /// Index containing the unexpected entry.
    index: usize,

    /// Raw unexpected entry.
    entry: usize,
  },

  /// A requested leaf virtual page already had a mapping.
  PageAlreadyMapped {
    /// Virtual page that was already mapped.
    address: usize,

    /// Existing raw leaf entry.
    entry: usize,
  },

  /// A page-table index was outside the 512-entry table.
  PageTableIndexOutOfRange {
    /// Invalid entry index.
    index: usize,
  },

  /// Runtime kernel mappings must stay in the canonical upper half.
  VirtualAddressNotKernel {
    /// Address outside the canonical kernel half.
    address: usize,
  },

  /// A runtime mapping targeted a reserved virtual page or the claimed heap.
  ReservedVirtualPage {
    /// Reserved virtual page.
    address: usize,
  },

  /// The bootstrap paging capability has already been claimed.
  BootstrapPagingAlreadyClaimed,

  /// The kernel heap's virtual reservation has already been claimed.
  KernelHeapAlreadyClaimed,
}

/// Returns the physical address of the active Sv39 root page table.
///
/// The physical address is reconstructed from the page number stored in the
/// active `satp` register.
///
/// # Errors
///
/// Returns [`PagingError::UnexpectedAddressTranslationMode`] if `satp` does not
/// select Sv39 translation.
fn active_root_table() -> Result<PhysAddr, PagingError> {
  let satp: usize;

  // SAFETY:
  // Reading `satp` does not modify memory or processor state.
  unsafe {
    asm!(
      "csrr {satp}, satp",
      satp = out(reg) satp,
      options(nomem, nostack),
    );
  }

  let mode = (satp >> SATP_MODE_SHIFT) & SATP_MODE_MASK;

  if mode != SATP_MODE_SV39 {
    return Err(PagingError::UnexpectedAddressTranslationMode { mode });
  }

  let root_ppn = satp & SATP_PPN_MASK;

  Ok(PhysAddr::new(root_ppn << PAGE_SHIFT))
}

/// Constructs an Sv39 page-table entry pointing at `physical_address`.
///
/// `flags` contains the desired Sv39 PTE flag bits.
///
/// # Errors
///
/// Returns [`PagingError::PhysicalAddressNotRepresentable`] if
/// `physical_address` cannot be encoded in an Sv39 PTE.
const fn page_table_entry(physical_address: PhysAddr, flags: usize) -> Result<usize, PagingError> {
  if physical_address.as_usize() > MAX_PHYSICAL_ADDRESS {
    return Err(PagingError::PhysicalAddressNotRepresentable {
      address: physical_address,
    });
  }

  let physical_page_number = physical_address.as_usize() >> PAGE_SHIFT;

  Ok((physical_page_number << PTE_PPN_SHIFT) | flags)
}

/// Returns the page-table frame referenced by a valid non-leaf entry.
///
/// # Errors
///
/// Returns [`PagingError::InvalidIntermediateEntry`] if `entry` is not a valid
/// Sv39 non-leaf entry.
const fn intermediate_frame(index: usize, entry: usize) -> Result<PhysFrame, PagingError> {
  if entry & PTE_VALID == 0 || entry & PTE_LEAF_MASK != 0 {
    return Err(PagingError::InvalidIntermediateEntry { index, entry });
  }

  let physical_page_number = (entry >> PTE_PPN_SHIFT) & SATP_PPN_MASK;

  let physical_address = PhysAddr::new(physical_page_number << PAGE_SHIFT);

  let Some(frame) = PhysFrame::from_start(physical_address) else {
    return Err(PagingError::InvalidIntermediateEntry { index, entry });
  };

  Ok(frame)
}

/// Invalidates the current hart's cached translation for `address`.
fn flush_address(address: VirtAddr) {
  // SAFETY:
  // `sfence.vma` only affects address-translation state on the current hart.
  unsafe {
    asm!(
      "sfence.vma {address}, zero",
      address = in(reg) address.as_usize(),
      options(nostack),
    );
  }
}

/// Invalidates all cached address translations on the current hart.
fn flush_all() {
  // SAFETY:
  // `sfence.vma` only affects address-translation state on the current hart.
  unsafe {
    asm!("sfence.vma zero, zero", options(nostack));
  }
}

/// Installs a complete replacement root on the current hart.
///
/// # Safety
///
/// `root` must be representable in `satp` and identify a fully initialized Sv39
/// hierarchy that preserves all live code, stack, trap, and borrowed-data
/// mappings. Its table frames must remain allocated while the address space
/// is active. The caller must consume the capability controlling the previous
/// root and expose only the replacement capability after this operation
/// completes.
unsafe fn activate_root(root: PhysFrame) {
  let satp = (SATP_MODE_SV39 << SATP_MODE_SHIFT) | (root.start_address().as_usize() >> PAGE_SHIFT);

  logging::debug!(
    Paging,
    "activating Sv39 root pa={:#x} satp={satp:#x} asid=0",
    root.start_address(),
  );

  // SAFETY:
  // The caller establishes the replacement mapping and allocation invariants.
  // The first fence orders completed table writes before the switch. The
  // second invalidates old translations, including those using ASID zero.
  // The assembly has a memory clobber. No fallible operation follows the satp
  // write.
  unsafe {
    asm!(
      "sfence.vma zero, zero",
      "csrw satp, {satp}",
      "sfence.vma zero, zero",
      satp = in(reg) satp,
      options(nostack),
    );
  }

  logging::debug!(
    Paging,
    "Sv39 root active pa={:#x}; local translations flushed",
    root.start_address(),
  );
}

/// Reads one entry from a page table that is currently mapped at `address`.
///
/// # Safety
///
/// `address` must be page-aligned and must map a complete readable
/// [`PageTable`] for the duration of this operation.
///
/// # Errors
///
/// Returns [`PagingError::PageTableIndexOutOfRange`] if `index` does not
/// identify an entry in the page table.
unsafe fn read_mapped_page_table_entry(
  address: VirtAddr,
  index: usize,
) -> Result<usize, PagingError> {
  if index >= PAGE_TABLE_ENTRIES {
    return Err(PagingError::PageTableIndexOutOfRange { index });
  }

  let table_pointer = ptr::with_exposed_provenance::<PageTable>(address.as_usize());

  // SAFETY:
  // The caller guarantees that `address` maps a complete `PageTable`, and the
  // bounds check above guarantees that `index` identifies one of its entries.
  let entry_pointer = unsafe { table_pointer.cast::<usize>().add(index) };

  // SAFETY:
  // `entry_pointer` refers to a valid entry within the mapped page table.
  Ok(unsafe { ptr::read_volatile(entry_pointer) })
}

/// Writes one entry in a page table that is currently mapped at `address`.
///
/// # Safety
///
/// `address` must be page-aligned and must map a complete writable
/// [`PageTable`] for the duration of this operation. The table must not be
/// concurrently accessed in a way that conflicts with this write.
///
/// # Errors
///
/// Returns [`PagingError::PageTableIndexOutOfRange`] if `index` does not
/// identify an entry in the page table.
unsafe fn write_mapped_page_table_entry(
  address: VirtAddr,
  index: usize,
  value: usize,
) -> Result<(), PagingError> {
  if index >= PAGE_TABLE_ENTRIES {
    return Err(PagingError::PageTableIndexOutOfRange { index });
  }

  let table_pointer = ptr::with_exposed_provenance_mut::<PageTable>(address.as_usize());

  // SAFETY:
  // The caller guarantees that `address` maps a complete writable `PageTable`,
  // and the bounds check above guarantees that `index` identifies one of its
  // entries.
  let entry_pointer = unsafe { table_pointer.cast::<usize>().add(index) };

  // SAFETY:
  // `entry_pointer` points to a writable PTE within the mapped page table.
  unsafe {
    ptr::write_volatile(entry_pointer, value);
  }

  Ok(())
}
