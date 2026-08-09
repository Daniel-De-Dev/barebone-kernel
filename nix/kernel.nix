/*
  Build the RISC-V kernel that runs as OpenSBI's S-mode next stage.

  The kernel is linked for a board-specific load address and memory
  region. The linker script ensures that the kernel's runtime memory
  footprint fits within that region.

  The linked ELF is retained for debugging, and llvm-objcopy also produces
  a flat binary for loading into memory. Platform-specific boot code places
  the binary at its load address and OpenSBI transfers control to it.
*/
{
  lib,
  pkgs,
  naersk',
  name,
  loadAddress,
  regionSize,
  release ? true,
}:
let
  kernelCrate = "kernel";
  buildProfile = if release then "release" else "debug";
  rustTarget = "riscv64gc-unknown-none-elf";

  loadAddressHex = "0x${lib.toHexString loadAddress}";
  regionSizeHex = "0x${lib.toHexString regionSize}";

  linkerScript = pkgs.replaceVars ../kernel/linker.ld.in {
    kernelAddress = loadAddressHex;
    kernelRegionSize = regionSizeHex;
  };
in
naersk'.buildPackage {
  pname = "${kernelCrate}-${name}-${buildProfile}";
  version = "0.1.0";
  src = ./..;

  inherit release;

  cargoBuildOptions =
    defaultOptions:
    defaultOptions
    ++ [
      "--package"
      kernelCrate
    ];

  CARGO_BUILD_TARGET = rustTarget;

  CARGO_TARGET_RISCV64GC_UNKNOWN_NONE_ELF_RUSTFLAGS = lib.concatStringsSep " " [
    "-Clink-arg=-T${linkerScript}"
  ];

  passthru = { inherit loadAddress regionSize; };

  nativeBuildInputs = [ pkgs.llvmPackages.bintools ];

  postInstall = ''
    elf="$out/bin/${kernelCrate}"
    bin_out="$out/bin/${kernelCrate}.bin"
    elf_out="$out/bin/${kernelCrate}.elf"

    llvm-objcopy -O binary "$elf" "$bin_out"

    mv -- "$elf" "$elf_out"
  '';
}
