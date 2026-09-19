//! RISC-V kernel bootstrap.
//!
//! Startup is split into two stages:
//!
//! - `_start` executes at the physical load address with translation disabled.
//!   It creates a minimal Sv39 root table containing temporary 1 GiB identity
//!   and higher-half mappings, enables paging, and jumps to the higher-half
//!   kernel entry.
//! - `__high_half_start` establishes the Rust execution environment by
//!   initializing `gp` and `sp`, clearing `.bss`, and entering [`crate::main`].
//!
//! The bootstrap intentionally uses only a single root page table and level-2
//! Sv39 leaves. The linker guarantees that the complete kernel boot footprint
//! fits within one physical 1 GiB region and the corresponding higher-half
//! virtual region.
//!
//! OpenSBI supplies the hart ID in `a0` and the physical device-tree address in
//! `a1`. Both registers are preserved until `main` is entered.
//!
//! The DTB's containing 1 GiB region is identity-mapped so the existing
//! physical pointer remains usable during early initialization. A DTB crossing
//! a 1 GiB boundary would require mapping the following region as well.

use core::arch::global_asm;

// Physical entry point and higher-half transition.
//
// This code runs before a Rust execution environment exists and must therefore
// remain stackless until `__high_half_start`.
global_asm!(
  r#"
  /* constants. */
  .equ PAGE_SIZE,             4096
  .equ PAGE_SHIFT,            12
  .equ GIGAPAGE_SHIFT,        30
  .equ PTE_BYTE_SHIFT,        3
  .equ VPN2_MASK,             0x1ff
  .equ PTE_VRWXAD,             0xcf
  .equ SATP_MODE_SV39,        8
  .equ SATP_MODE_SHIFT,       60

  /*
   * Physical bootstrap.
   *
   * Entry:
   *   a0 = hart ID
   *   a1 = physical DTB address
   *
   * Persistent register state:
   *   t0 = physical root page-table address
   *   t3 = physical kernel gigapage base
   *   t4 = kernel gigapage leaf PTE
   *   t6 = virtual address of __high_half_start
   */

  .section .boot.text, "ax"
  .global _start

_start:
  lla t0, __boot_page_table
  mv t1, t0

  li t2, PAGE_SIZE
  add t2, t0, t2

.Lclear_boot_page_table:
  bgeu t1, t2, .Lboot_page_table_cleared

  sd zero, 0(t1)
  addi t1, t1, 8
  j .Lclear_boot_page_table

.Lboot_page_table_cleared:
  /*
   * The high entry is outside the range of a low PC-relative address.
   * Load its full virtual address through a nearby bootstrap literal.
   */
  lla t5, .Lhigh_half_start_address
  ld t6, 0(t5)

  /* Round _start down to the containing physical 1 GiB region. */
  lla t3, _start
  srli t3, t3, GIGAPAGE_SHIFT
  slli t3, t3, GIGAPAGE_SHIFT

  /*
   * Construct an RWXAD level-2 leaf for that region.
   *
   * PPN occupies PTE bits 10+, so:
   *   (physical_address >> 12) << 10 == physical_address >> 2
   */
  srli t4, t3, 2
  ori t4, t4, PTE_VRWXAD

  /*
   * Identity-map the kernel gigapage so the current PC survives satp
   * activation.
   */
  srli t5, t3, GIGAPAGE_SHIFT
  andi t5, t5, VPN2_MASK
  slli t5, t5, PTE_BYTE_SHIFT

  add t5, t0, t5
  sd t4, 0(t5)

  /* Map the same physical gigapage into the kernel's higher-half VPN[2]. */
  srli t5, t6, GIGAPAGE_SHIFT
  andi t5, t5, VPN2_MASK
  slli t5, t5, PTE_BYTE_SHIFT

  add t5, t0, t5
  sd t4, 0(t5)

  /*
   * Keep the firmware DTB directly accessible. If it shares the kernel gigapage
   * this simply rewrites the same identity entry.
   */
  srli t1, a1, GIGAPAGE_SHIFT
  slli t1, t1, GIGAPAGE_SHIFT

  srli t2, t1, 2
  ori t2, t2, PTE_VRWXAD

  srli t5, t1, GIGAPAGE_SHIFT
  andi t5, t5, VPN2_MASK
  slli t5, t5, PTE_BYTE_SHIFT

  add t5, t0, t5
  sd t2, 0(t5)

  /* Order the PTE stores before the MMU begins walking the new table. */
  sfence.vma zero, zero

  /*
   * satp = Sv39 | root PPN.
   *
   * ASID remains zero during bootstrap.
   */
  srli t1, t0, PAGE_SHIFT

  li t2, SATP_MODE_SV39
  slli t2, t2, SATP_MODE_SHIFT

  or t1, t1, t2
  csrw satp, t1

  /* Discard stale translation state after installing the new address space. */
  sfence.vma zero, zero

  /* Continue through the higher-half alias of the kernel image. */
  jr t6


  /*
   * Nearby storage for the otherwise unreachable high virtual entry address.
   */

  .section .boot.rodata, "a"
  .balign 8

.Lhigh_half_start_address:
  .dword __high_half_start


  /*
   * Higher-half Rust environment.
   *
   * Normal linker symbols are virtual addresses from this point onward.
   */

  .section .text.init, "ax"
  .global __high_half_start

__high_half_start:
  /*
   * Prevent relaxation from assuming gp is already initialized while loading
   * __global_pointer$ itself.
   */
  .option push
  .option norelax
  la gp, __global_pointer$
  .option pop

  la sp, _stack_end

  /* The bootstrap page table is outside .bss and must remain intact. */
  la t0, _bss_start
  la t1, _bss_end

.Lbss_loop:
  bgeu t0, t1, .Lbss_done

  sd zero, 0(t0)
  addi t0, t0, 8
  j .Lbss_loop

.Lbss_done:
  /* a0 and a1 still carry OpenSBI's hart ID and physical DTB address. */
  tail main
  "#
);
