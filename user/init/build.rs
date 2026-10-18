//! Selects the fixed-address linker layout for the standalone user executable.

/// Emits the linker script path for this binary.
fn main() {
  println!("cargo:rerun-if-changed=linker.ld");
  println!(
    "cargo:rustc-link-arg=-T{}/linker.ld",
    env!("CARGO_MANIFEST_DIR")
  );
}
