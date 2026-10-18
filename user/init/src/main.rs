//! Standalone integer-only user program exercising scalar syscalls and timers.

#![no_std]
#![no_main]

use core::{
  arch::{asm, global_asm},
  hint::black_box,
  panic::PanicInfo,
};

global_asm!(
  r#"
  .section .text.start, "ax"
  .global _start
_start:
  .option push
  .option norelax
  la gp, __global_pointer$
  .option pop
  tail {main}
  "#,
  main = sym user_main,
);

/// Prints a greeting, retains stack data across timer delivery, and exits.
extern "C" fn user_main() -> ! {
  write(b"Hello from user mode\n");

  let mut values = [0x1357_9bdf_2468_ace0u64; 32];
  let start = syscall(user_abi::TIMER_TICKS, 0);

  while syscall(user_abi::TIMER_TICKS, 0).wrapping_sub(start) < 3 {
    for _ in 0..10_000 {
      for value in black_box(&mut values) {
        *value = black_box(value.rotate_left(1));
        *value = black_box(value.rotate_right(1));
      }
    }
  }

  if black_box(values)
    .iter()
    .any(|value| *value != 0x1357_9bdf_2468_ace0)
  {
    exit(1);
  }

  write(b"User timer resume confirmed\n");
  exit(0)
}

/// Writes constant bytes through the scalar console syscall.
fn write(bytes: &[u8]) {
  for byte in bytes {
    if syscall(user_abi::CONSOLE_WRITE_BYTE, usize::from(*byte)) != 0 {
      exit(2);
    }
  }
}

/// Executes a one-argument syscall and returns its scalar result.
fn syscall(number: usize, argument: usize) -> usize {
  let result: usize;

  // SAFETY:
  // ecall enters the supervisor syscall handler. The scalar ABI preserves all
  // integer registers except the returned a0 and does not change the user stack.
  unsafe {
    asm!(
      "ecall",
      in("a7") number,
      inlateout("a0") argument => result,
      options(nostack),
    );
  }

  result
}

/// Requests process termination and traps if the non-returning call returns.
fn exit(status: usize) -> ! {
  let _result = syscall(user_abi::EXIT, status);

  // SAFETY: unimp deliberately raises an illegal-instruction exception.
  unsafe {
    asm!("unimp", options(noreturn, nostack));
  }
}

/// Terminates the process when Rust detects an unrecoverable error.
#[panic_handler]
fn panic(_info: &PanicInfo<'_>) -> ! {
  exit(3)
}
