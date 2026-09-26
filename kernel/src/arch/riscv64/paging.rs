//! RISC-V Sv39 paging support.
//!
//! This module provides address-translation helpers and temporary mappings used
//! while the kernel is running under its bootstrap Sv39 address space.

use core::{arch::asm, ptr};

use crate::memory::{PhysAddr, VirtAddr};

/// Lowest canonical virtual address in the upper half of Sv39.
const SV39_HIGH_HALF_BASE: usize = 0xffff_ffc0_0000_0000;

/// Number of low address bits forming a page offset.
const PAGE_SHIFT: usize = 12;

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

/// Size in bytes of one Sv39 page-table entry.
const PTE_SIZE: usize = size_of::<u64>();

/// PTE bit marking an entry as valid.
const PTE_VALID: usize = 1 << 0;

/// PTE bit permitting reads through a leaf mapping.
const PTE_READ: usize = 1 << 1;

/// PTE bit recording that a leaf mapping has been accessed.
const PTE_ACCESSED: usize = 1 << 6;

/// Flags for a read-only boot-time FDT leaf mapping.
const BOOT_FDT_PTE_FLAGS: usize = PTE_VALID | PTE_READ | PTE_ACCESSED;

/// First virtual address of the boot-time FDT mapping window.
const BOOT_FDT_WINDOW_BASE: usize = SV39_HIGH_HALF_BASE;

/// Root-table index containing the start of the boot-time FDT window.
const BOOT_FDT_WINDOW_VPN2: usize = (BOOT_FDT_WINDOW_BASE >> GIGAPAGE_SHIFT) & VPN_MASK;

/// Number of consecutive level-2 leaves available to the boot-time FDT window.
///
/// Five leaves cover the largest `u32`-sized FDT even when it begins at the
/// end of its first 1 GiB region.
const BOOT_FDT_WINDOW_GIGAPAGES: usize = 5;

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

/// Maps the physical region containing `dtb` into a higher-half boot window.
///
/// The mapping preserves `dtb`'s offset within its first 1 GiB physical region,
/// so the returned virtual address refers to the same byte as `dtb`. The window
/// consists of consecutive read-only, non-executable Sv39 level-2 leaves.
///
/// The active root table must be accessible through an identity mapping while
/// this function executes.
///
/// # Errors
///
/// Returns [`PagingError::UnexpectedAddressTranslationMode`] if Sv39 is not
/// active, [`PagingError::RootEntryInUse`] if the required virtual window is
/// occupied, or [`PagingError::PhysicalAddressNotRepresentable`] if the DTB
/// window requires a physical address that cannot be represented by Sv39.
#[expect(
  clippy::arithmetic_side_effects,
  reason = "window indices are statically bounded, the active root comes from \
            the satp PPN, and the DTB address is validated before \
            physical-address arithmetic"
)]
pub(crate) fn map_bootstrap_fdt(dtb: PhysAddr) -> Result<VirtAddr, PagingError> {
  let boot_root = active_root_table()?;

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
    let entry_offset = root_index * PTE_SIZE;

    let entry_address = PhysAddr::new(boot_root.as_usize() + entry_offset);

    let entry_pointer = ptr::with_exposed_provenance::<usize>(entry_address.as_usize());

    // SAFETY:
    // The active bootstrap root is located inside the physical kernel
    // gigapage, which remains identity-mapped while this function executes.
    let existing = unsafe { ptr::read_volatile(entry_pointer) };

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

    let entry_offset = root_index * PTE_SIZE;

    let entry_address = PhysAddr::new(boot_root.as_usize() + entry_offset);

    let physical_page_number = physical_address.as_usize() >> PAGE_SHIFT;

    let entry = (physical_page_number << PTE_PPN_SHIFT) | BOOT_FDT_PTE_FLAGS;

    let entry_pointer = ptr::with_exposed_provenance_mut::<usize>(entry_address.as_usize());

    // SAFETY:
    // The active bootstrap root remains identity-mapped, and the selected
    // entry was verified to be unused before any mappings were installed.
    unsafe {
      ptr::write_volatile(entry_pointer, entry);
    }
  }

  // SAFETY:
  // `sfence.vma` makes the preceding PTE updates visible to address translation
  // on the current hart.
  unsafe {
    asm!("sfence.vma zero, zero", options(nostack));
  }

  let dtb_offset = dtb.as_usize() & GIGAPAGE_MASK;

  let virtual_address = BOOT_FDT_WINDOW_BASE + dtb_offset;

  Ok(VirtAddr::new(virtual_address))
}
