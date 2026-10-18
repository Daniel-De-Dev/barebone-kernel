//! Loading and execution of one embedded process in the active address space.

mod image;
mod syscall;

use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};

pub(crate) use syscall::handle as handle_syscall;

use crate::{
  arch, logging,
  memory::{self, FRAME_SIZE, PagePermissions, VirtAddr},
};

/// Header-prefixed executable bytes embedded in immutable kernel storage.
static INIT_IMAGE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/init.bin"));

/// Exclusive upper boundary of the user stack.
const USER_STACK_TOP: usize = 0x4000_0000;

/// Byte capacity of the user stack and its dedicated kernel stack.
const STACK_SIZE: usize = 16 * 1024;

/// Number of 16-byte aligned elements backing the supervisor stack.
const KERNEL_STACK_ELEMENTS: usize = STACK_SIZE / size_of::<u128>();

/// Inclusive lower boundary of the user stack.
const USER_STACK_BOTTOM: usize = USER_STACK_TOP - STACK_SIZE;

/// Exclusive image bound, preserving an unmapped page below the user stack.
const IMAGE_LIMIT: usize = USER_STACK_BOTTOM - FRAME_SIZE;

/// Whether the embedded-process mapping slot has been claimed.
static CLAIMED: AtomicBool = AtomicBool::new(false);

/// Failures while loading the embedded process.
#[derive(Debug)]
pub(crate) enum LoadError {
  /// No embedded image was supplied to the kernel build.
  MissingImage,
  /// Image metadata or payload bounds are invalid.
  InvalidImage,
  /// The process slot has already been claimed.
  AlreadyClaimed,
  /// The dedicated kernel stack could not be allocated.
  KernelStackAllocation,
  /// Global paging and physical allocation state could not be borrowed.
  Memory(memory::MemoryAccessError),
  /// A physical frame or user mapping could not be established.
  Paging(arch::PagingError),
}

impl From<arch::PagingError> for LoadError {
  fn from(error: arch::PagingError) -> Self {
    Self::Paging(error)
  }
}

/// A loaded user context with an exclusively owned supervisor stack.
pub(crate) struct Process {
  /// Integer-only execution state prepared for the first user return.
  context: arch::UserContext,
  /// Stable, 16-byte-aligned heap storage for kernel trap handling.
  kernel_stack: Vec<u128>,
}

/// Loads the embedded executable and zeroed user stack exactly once.
///
/// The image is copied into fresh RX user pages. The user stack is backed by
/// zeroed RW/NX pages. Addresses outside those mappings provide no allocation.
/// Failures after claiming the slot retain the claim and any mapped frames.
///
/// # Errors
///
/// Returns an error for an invalid image, repeated load, kernel stack exhaustion,
/// unavailable memory state, physical frame exhaustion, or failed user mappings.
pub(crate) fn load_init() -> Result<Process, LoadError> {
  let image = image::Image::parse(INIT_IMAGE, IMAGE_LIMIT)?;
  if CLAIMED.swap(true, Ordering::Relaxed) {
    return Err(LoadError::AlreadyClaimed);
  }

  let mut kernel_stack = Vec::new();
  kernel_stack
    .try_reserve_exact(KERNEL_STACK_ELEMENTS)
    .map_err(|_| LoadError::KernelStackAllocation)?;
  kernel_stack.resize(KERNEL_STACK_ELEMENTS, 0);

  memory::with_paging(|paging, frames| {
    image.load(paging, frames)?;
    for address in (USER_STACK_BOTTOM..USER_STACK_TOP).step_by(FRAME_SIZE) {
      image::map_page(
        paging,
        frames,
        VirtAddr::new(address),
        &[],
        PagePermissions::ReadWrite,
      )?;
    }
    Ok::<(), LoadError>(())
  })
  .map_err(LoadError::Memory)??;

  logging::debug!(
    Process,
    "Embedded image loaded (image_bytes={}, base={:#x}, entry={:#x}, user_stack=[{USER_STACK_BOTTOM:#x}, {USER_STACK_TOP:#x}))",
    INIT_IMAGE.len(),
    image.base.as_usize(),
    image.entry.as_usize(),
  );
  logging::info!(Process, "Embedded user process ready");

  Ok(Process {
    context: arch::UserContext::new(image.entry, VirtAddr::new(USER_STACK_TOP)),
    kernel_stack,
  })
}

impl Process {
  /// Consumes the process and enters user mode with its dedicated kernel stack.
  ///
  /// Stack storage is retained permanently because execution does not return.
  ///
  /// # Safety
  ///
  /// The process's page-table root must remain active on its owning hart.
  /// Trap handling and every enabled interrupt source must be ready there,
  /// and no interrupt-sensitive borrow may survive the transition.
  pub(crate) unsafe fn run(self) -> ! {
    let stack = self.kernel_stack.leak();
    let top = stack.as_mut_ptr().cast::<u8>().wrapping_add(STACK_SIZE);
    logging::info!(Process, "Entering user mode");

    // SAFETY:
    // Loading established executable user entry and writable user stack pages.
    // The leaked stack has STACK_SIZE writable supervisor-only bytes and a
    // 16-byte-aligned top, and no other code accesses its storage. The active
    // root, owning hart, and handler readiness are guaranteed by the caller.
    unsafe {
      arch::enter_user(self.context, VirtAddr::new(top.addr()));
    }
  }
}

/// Records the process exit status and resumes interruptible supervisor idling.
pub(crate) fn exit(status: usize) -> ! {
  let ticks = arch::user_timer_ticks();
  if status == 0 {
    logging::info!(
      Process,
      "User process exited (status=0, user_timer_interrupts={ticks})"
    );
  } else {
    logging::warn!(
      Process,
      "User process exited (status={status}, user_timer_interrupts={ticks})"
    );
  }
  idle()
}

/// Reports a user exception and ends the process without retrying the fault.
pub(crate) fn fault(cause: usize, pc: usize, value: usize) -> ! {
  logging::error!(
    Process,
    "User process fault (cause={cause:#x}, pc={pc:#x}, value={value:#x}), terminating"
  );
  idle()
}

/// Parks supervisor execution while allowing configured interrupt handlers to run.
fn idle() -> ! {
  logging::info!(Process, "Kernel idle after user process");
  // SAFETY:
  // This runs on the dedicated supervisor stack after a user trap. Trap entry
  // cleared its stack-switch scratch state, the timer handler remains ready,
  // and no memory-state or frame-window borrow survives this call.
  unsafe {
    arch::idle();
  }
}
