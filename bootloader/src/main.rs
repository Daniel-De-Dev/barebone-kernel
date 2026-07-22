#![no_main]
#![no_std]

// TODO: refactor and clean up the bootloader to sperate files
// TODO: Define tests?

use core::{
  fmt::{self, Write},
  panic::PanicInfo,
  time::Duration,
};

use uefi::{
  boot::{self, image_handle},
  prelude::*,
  print, println,
  proto::media::file::{File, FileAttribute, FileMode},
};

struct Config;
impl Config {
  // Automatically set level to 'Trace' in debug mode
  // and 'Info' in release mode
  #[cfg(debug_assertions)]
  pub const LOG_LEVEL: log::LevelFilter = log::LevelFilter::Trace;

  #[cfg(not(debug_assertions))]
  pub const LOG_LEVEL: log::LevelFilter = log::LevelFilter::Info;
}

// Implement writing bytes to Serial
struct SerialWriter;

// TODO: Look into concurrency regarding (where printing could become shuffled)
// not really sure how applicable in bootloader
impl fmt::Write for SerialWriter {
  cfg_select! {
    target_arch = "riscv64" => {
      fn write_str(&mut self, s: &str) -> fmt::Result {
        let bytes = s.as_bytes();
      let phys_bytes = sbi_rt::Physical::new(bytes.len(), bytes.as_ptr() as usize, 0);
        sbi_rt::console_write(phys_bytes);
        Ok(())
      }
    }
    // TODO: fix the lsp to know correct branch (throwing error on last catch-all)

    // Explicit compiler error for any other architecture
    _ => {
      fn write_str(&mut self, _s: &str) -> fmt::Result {
        compile_error!(concat!(
          "Serial output is not implemented for the current target architecture",
          "\nFix: Add a new case to the cfg_select! block which implements it."
        ));
        Ok(())
      }
    }
  }
}

// Helper to get CPU cycle count
fn get_timestamp() -> u64 {
  cfg_select! {
    target_arch = "riscv64" => {
      riscv::register::time::read() as u64
          }
    _ => {
      compile_error!(concat!(
        "Retrieving timestamp from CPU has not been implemented for current architecture",
        "\nFix: figure out and implement the equivalent of getting cycle/time counter"
      ));
      0
    }
  }
}

struct Logger;
impl log::Log for Logger {
  fn enabled(&self, metadata: &log::Metadata) -> bool {
    metadata.level() <= Config::LOG_LEVEL
  }

  fn log(&self, record: &log::Record) {
    if self.enabled(record.metadata()) {
      let _ = writeln!(
        SerialWriter,
        "[{:>16}] [{:<5}] [{:<14}] ({:>16}:{:<3}) {}",
        get_timestamp(),
        record.level(),
        record.target(),
        record.file().unwrap_or("<unknown file>"),
        record.line().unwrap_or_default(),
        record.args()
      );
    }
  }

  fn flush(&self) {
    // NOTE: Serial ports do not have software-managed buffers that require flushing.
    // intentionally left empty.
  }
}

static LOGGER: Logger = Logger;

// Implement panic handler
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
  let _ = writeln!(SerialWriter, "\n!!! PANIC !!!\n{}", info);

  // WARN: Might not be ideal to use print! to write to stdout while panicking
  print!("\n!!!PANIC!!!\n{}", info);

  loop {
    cfg_select! {
      target_arch = "riscv64" => {
        riscv::asm::wfi();
      }
      _ => {
        compile_error!(concat!(
          "Halting the CPU for when a panic happens has not been implemented for current target architecture",
          "\nFix: Find and add the instruction to achieve the equivalent"
        ));
      }
    }
  }
}

#[entry]
fn main() -> Status {
  log::set_logger(&LOGGER).expect("Failed to set logger");
  log::set_max_level(Config::LOG_LEVEL);

  log::info!("Hello from the Serial Port! Logging initilzed!");
  println!("Hello from bootloader, starting running!");

  log::debug!("Locating the file system bootloader was loaded from");
  let mut boot_fs =
    uefi::boot::get_image_file_system(image_handle()).expect("Failed to get file system");

  log::debug!("Opening the file system volume");
  let mut root = boot_fs.open_volume().expect("Failed to open volume");

  log::debug!("Opening kernel.bin");
  let _kernel_file = root
    .open(
      cstr16!("kernel.bin"),
      FileMode::Read,
      FileAttribute::empty(),
    )
    .expect("Failed to open kernel.bin")
    .into_regular_file()
    .expect("kernel.bin was not a regular file");

  boot::stall(Duration::from_secs(10));
  Status::SUCCESS
}
