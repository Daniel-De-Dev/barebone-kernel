{
  inputs,
  lib,
  pkgs,
}:
let
  riscvPkgs = pkgs.pkgsCross.riscv64;
  crossCompile = riscvPkgs.stdenv.cc.targetPrefix;

  toHex = value: "0x${lib.toHexString value}";

  /*
    Build an OpenSBI firmware image for the generic FDT platform.

    `name` identifies the target and forms part of the package name.

    `image` selects the OpenSBI firmware image to install.

    `textStart` sets FW_TEXT_START.

    `extraEnv` provides firmware-type-specific OpenSBI build options.
  */
  mkOpenSBI =
    {
      name,
      image,
      textStart,
      extraEnv ? { },
    }:
    riscvPkgs.stdenv.mkDerivation {
      pname = "opensbi-${name}";
      version = inputs.src-opensbi.shortRev or "dirty";
      src = inputs.src-opensbi;

      nativeBuildInputs = [ pkgs.python3 ];

      postPatch = ''
        patchShebangs scripts
      '';

      enableParallelBuilding = true;

      env = {
        ARCH = "riscv";
        CROSS_COMPILE = crossCompile;
        PLATFORM = "generic";
        FW_TEXT_START = toHex textStart;
      }
      // extraEnv;

      installPhase = ''
        runHook preInstall

        install -Dm0644 \
          build/platform/generic/firmware/${image}.bin \
          "$out/${image}.bin"

        runHook postInstall
      '';
    };
in
{
  /*
    Build an OpenSBI FW_DYNAMIC image.

    The next-stage address is supplied at runtime by the boot stage
    preceding OpenSBI.

    # TODO: Update permalinks (github.com/...) to align with flake.lock (for all links, update flake before)
    Sources:
    https://github.com/riscv-software-src/opensbi/blob/c0f87f10d1bfb9e72a84ddfafb5604ee1bfe9d04/docs/firmware/fw_dynamic.md
  */
  mkDynamic =
    { name, textStart }:
    mkOpenSBI {
      inherit name textStart;
      image = "fw_dynamic";
    };

  /*
    Build an OpenSBI FW_JUMP image.

    `jumpAddress` is the fixed entry address of the S-mode stage executed
    after OpenSBI.

    `fdtPath` and `fdtAddress` may be supplied together to embed an FDT
    and relocate it before entering the next stage.
  */
  mkJump =
    {
      name,
      textStart,
      jumpAddress,
      fdtPath ? null,
      fdtAddress ? null,
    }:
    assert lib.assertMsg (
      (fdtPath == null) == (fdtAddress == null)
    ) "OpenSBI FDT path and address must be provided together";
    mkOpenSBI {
      inherit name textStart;

      image = "fw_jump";

      extraEnv = {
        FW_JUMP = "y";
        FW_JUMP_ADDR = toHex jumpAddress;
      }
      // lib.optionalAttrs (fdtPath != null) {
        FW_FDT_PATH = fdtPath;
        FW_JUMP_FDT_ADDR = toHex fdtAddress;
      };
    };
}
