/*
  Build the RISC-V bootloader as a UEFI application.

  Rust cannot currently produce the RISC-V PE/COFF objects needed by a native
  UEFI target. The build hence has this pipeline:

  Rust source is compiled to a position-independent RISC-V ELF then `elf2efi.py`
  convert the ELF to a PE32+ UEFI application

  The `uefi` crate provides the UEFI ABI and entry point, while
  `elf2efi.py` does the ELF-to-PE/COFF conversion.

  Sources:
  https://discourse.llvm.org/t/rfc-uefi-driver-support-uefi-target/73261
  https://github.com/systemd/systemd/blob/main/tools/elf2efi.py
  https://uefi.org/specs/UEFI/2.11/02_Overview.html#uefi-images
*/
{
  lib,
  pkgs,
  naersk',
  rustToolchain,
  release ? true,
  ...
}:
let
  bootloaderCrate = "bootloader";
  buildProfile = if release then "release" else "debug";
  rustTarget = "riscv64gc-unknown-none-elf";
  efiEntryPoint = "efi_main";
  efiFileName = "BOOTRISCV64.EFI";

  python = pkgs.python3.withPackages (pythonPackages: [
    pythonPackages.pyelftools
  ]);

  elf2efi = "${pkgs.systemd.src}/tools/elf2efi.py";
in
naersk'.buildPackage {
  pname = "${bootloaderCrate}-${buildProfile}";
  version = "0.1.0";
  src = ./..;

  inherit release;

  additionalCargoLock = "${rustToolchain}/lib/rustlib/src/rust/library/Cargo.lock";

  cargoBuildOptions =
    defaultOptions:
    defaultOptions
    ++ [
      "--package"
      bootloaderCrate
      "-Z"
      "build-std=core,alloc"
    ];

  CARGO_BUILD_TARGET = rustTarget;

  CARGO_TARGET_RISCV64GC_UNKNOWN_NONE_ELF_RUSTFLAGS = lib.concatStringsSep " " [
    "-Crelocation-model=pic"
    "-Clink-arg=--pie"
    "-Clink-arg=--entry=${efiEntryPoint}"
  ];

  nativeBuildInputs = [
    pkgs.llvmPackages.llvm
    python
  ];

  # Perform the conversion from ELF to EFI
  postInstall = ''
    elf="$out/bin/${bootloaderCrate}"
    efi="$out/bin/${efiFileName}"

    "${python}/bin/python" \
      "${elf2efi}" \
      "$elf" \
      "$efi"

    rm -- "$elf"
  '';

  # Validate the installed artifact as a sanity check
  overrideMain =
    _previousAttrs:
    let
      peCoff = {
        machineRiscV64 = "0x5064";
        pe32PlusMagic = "0x20B";
        efiApplicationSubsystem = "0xA";
      };
    in
    {
      doInstallCheck = true;

      installCheckPhase = ''
        runHook preInstallCheck

        efi="$out/bin/${efiFileName}"
        headers="$TMPDIR/${bootloaderCrate}-efi-headers.txt"
        non_zero_integer='(0x[1-9a-f][0-9a-f]*|[1-9][0-9]*)'

        llvm-readobj --file-headers "$efi" > "$headers"

        assert_header() {
          local requirement="$1"
          local pattern="$2"

          if grep -Eiq "$pattern" "$headers"; then
            return
          fi

          echo "error: invalid UEFI image: $requirement" >&2
          echo "PE/COFF headers:" >&2
          cat "$headers" >&2
          exit 1
        }

        assert_header \
          "machine must be RISC-V 64-bit (${peCoff.machineRiscV64})" \
          "Machine: .*${peCoff.machineRiscV64}"

        assert_header \
          "optional header must identify a PE32+ image (${peCoff.pe32PlusMagic})" \
          "Magic: .*${peCoff.pe32PlusMagic}"

        assert_header \
          "subsystem must identify an EFI application (${peCoff.efiApplicationSubsystem})" \
          "Subsystem: .*${peCoff.efiApplicationSubsystem}"

        assert_header \
          "AddressOfEntryPoint must be non-zero" \
          "AddressOfEntryPoint: $non_zero_integer"

        assert_header \
          "the base-relocation table must be non-empty" \
          "BaseRelocationTableSize: $non_zero_integer"

        rm -- "$headers"

        runHook postInstallCheck
      '';
    };
}
