//! Rust global allocation backed by an owned kernel heap and paging state.

use core::{
  alloc::{GlobalAlloc, Layout},
  cell::UnsafeCell,
  ptr::{self, NonNull},
  sync::atomic::{AtomicBool, Ordering},
};

use super::{BootFrameAllocator, KernelHeap};
use crate::arch;

/// Failures while installing the global allocator's memory state.
#[derive(Debug)]
pub(crate) enum AllocatorInitError {
  /// Another operation currently holds the memory state.
  Busy,

  /// Memory state has already been installed.
  AlreadyInitialized,
}

/// Owned memory-management capabilities.
struct MemoryState {
  /// Virtual allocation policy and progress.
  heap: KernelHeap,

  /// Exclusive control of the active kernel address space.
  paging: arch::KernelPaging,

  /// Physical frame allocation and its retained DTB view.
  frames: BootFrameAllocator<'static>,
}

/// Non-reentrant access to optional global memory state.
///
/// Allocation returns null before initialization or while the state is held.
/// State access masks local supervisor interrupts and never waits.
struct KernelAllocator {
  /// Whether an operation holds exclusive access to `state`.
  busy: AtomicBool,

  /// Installed once, then accessed only while `busy` is held.
  state: UnsafeCell<Option<MemoryState>>,
}

/// Releases an acquired memory-state gate when dropped.
struct StateGuard<'a> {
  /// Flag held by this operation.
  busy: &'a AtomicBool,
}

impl Drop for StateGuard<'_> {
  fn drop(&mut self) {
    self.busy.store(false, Ordering::Release);
  }
}

// SAFETY:
// The atomic gate serializes every access to the UnsafeCell. Interrupt masking
// prevents asynchronous re-entry on its owning hart, and recursive requests
// fail before touching state. The initialization contract restricts allocation
// to that hart with the stored paging root active.
unsafe impl Sync for KernelAllocator {}

impl KernelAllocator {
  /// Runs an operation with exclusive state access and supervisor interrupts masked.
  ///
  /// Returns `None` immediately if the state is already held. Releases the gate
  /// before restoring the previous interrupt-enable state.
  fn with_state<R>(&self, operation: impl FnOnce(&mut Option<MemoryState>) -> R) -> Option<R> {
    let _interrupts: arch::InterruptGuard = arch::disable_interrupts();

    if self
      .busy
      .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
      .is_err()
    {
      return None;
    }

    let _state_guard = StateGuard { busy: &self.busy };

    // SAFETY:
    // The successful atomic acquisition gives this operation exclusive access.
    // Both guards outlive the borrow, and recursive requests cannot acquire it.
    let state = unsafe { &mut *self.state.get() };

    Some(operation(state))
  }
}

// SAFETY:
// KernelHeap returns distinct, aligned, fully backed allocations or an error,
// which is translated to null. All heap and backing state is exclusively
// accessed through with_state under the initialization contract. This path
// must not allocate recursively or unwind. Default zeroing and reallocation
// operate on the returned blocks. Deallocation retains their storage.
unsafe impl GlobalAlloc for KernelAllocator {
  unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
    self
      .with_state(|state| {
        let Some(state) = state.as_mut() else {
          return ptr::null_mut();
        };

        let mut backing = state.paging.heap_backing(&mut state.frames);

        state
          .heap
          .allocate(layout, &mut backing)
          .map_or(ptr::null_mut(), NonNull::as_ptr)
      })
      .unwrap_or(ptr::null_mut())
  }

  /// Retains the block's storage without reclaiming virtual space or frames.
  unsafe fn dealloc(&self, _pointer: *mut u8, _layout: Layout) {}
}

/// Global Rust allocator.
#[global_allocator]
static ALLOCATOR: KernelAllocator = KernelAllocator {
  busy: AtomicBool::new(false),
  state: UnsafeCell::new(None),
};

/// Installs owned heap, paging, and physical allocation state exactly once.
///
/// Initialization does not allocate. Supplied capabilities are consumed even
/// on failure. Previously installed state is never replaced.
///
/// # Safety
///
/// `heap` must be the heap claimed from `paging`. Its root must remain active
/// on its owning hart for every subsequent global allocation. Other harts must
/// not use this allocator. The DTB borrowed by `frames` must remain readable
/// and unchanged at its retained alias and excluded from physical allocation.
///
/// # Errors
///
/// Returns an error if the memory state is busy or already initialized.
pub(crate) unsafe fn initialize(
  heap: KernelHeap,
  paging: arch::KernelPaging,
  frames: BootFrameAllocator<'static>,
) -> Result<(), AllocatorInitError> {
  let state = MemoryState {
    heap,
    paging,
    frames,
  };

  ALLOCATOR
    .with_state(|slot| {
      if slot.is_some() {
        return Err(AllocatorInitError::AlreadyInitialized);
      }

      *slot = Some(state);
      Ok(())
    })
    .unwrap_or(Err(AllocatorInitError::Busy))
}
