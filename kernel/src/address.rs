//! Address types used by the kernel.
//!
//! This module distinguishes addresses by the address space in which they are
//! interpreted.

use core::fmt;

/// A physical memory address.
///
/// `PhysAddr` represents an address in the machine's physical address space.
/// Constructing a `PhysAddr` does not guarantee that the address refers to
/// usable memory, is mapped into the kernel's address space, or is safe to
/// dereference.
///
/// Physical addresses must therefore not be treated as pointers unless the
/// current memory configuration explicitly permits doing so.
#[repr(transparent)]
pub(super) struct PhysAddr(usize);

impl PhysAddr {
  /// Constructs a physical address from its machine-sized integer
  /// representation.
  ///
  /// No validation of the represented address is performed.
  pub(super) const fn new(address: usize) -> Self {
    Self(address)
  }

  /// Returns the machine-sized integer representation of this physical
  /// address.
  pub(crate) const fn as_usize(self) -> usize {
    self.0
  }
}

impl fmt::LowerHex for PhysAddr {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    fmt::LowerHex::fmt(&self.0, formatter)
  }
}

/// An address in the kernel's current virtual address space.
///
/// `VirtAddr` represents an address as interpreted by the processor for
/// instruction fetches and memory accesses under the current address-translation
/// configuration.
///
/// Will be refined once needed.
pub(super) struct VirtAddr(usize);
