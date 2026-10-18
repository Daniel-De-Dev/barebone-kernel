//! Stages a prebuilt user image and requires its presence when linking.

use std::{env, fs, path::PathBuf};

/// Stages the user payload, permitting its omission for source-only checks.
#[expect(
  clippy::expect_used,
  reason = "unavailable build inputs must fail the build"
)]
fn main() {
  println!("cargo:rerun-if-env-changed=USER_IMAGE");
  let directory = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo must supply OUT_DIR"));
  let output = directory.join("init.bin");

  if let Some(source) = env::var_os("USER_IMAGE") {
    let source = PathBuf::from(source);
    println!("cargo:rerun-if-changed={}", source.display());
    let bytes = fs::read(source).expect("embedded user image must be readable");
    fs::write(output, bytes).expect("build output must be writable");
  } else {
    fs::write(output, []).expect("build output must be writable");
    let script = directory.join("missing-image.ld");
    fs::write(
      &script,
      "ASSERT(0, \"kernel linking requires USER_IMAGE\")\n",
    )
    .expect("build output must be writable");
    println!("cargo:rustc-link-arg=-T{}", script.display());
  }
}
