_: {
  perSystem =
    {
      config,
      pkgs,
      lib,
      ...
    }:
    let
      # TODO: Document boot process for qemu in more detail
      # https://github.com/riscv-software-src/opensbi/blob/master/docs/platform/qemu_virt.md
      # https://docs.u-boot.org/en/stable/board/emulation/qemu-riscv.html
      # https://www.qemu.org/docs/master/system/riscv/virt.html#running-u-boot

      boards = import ./boards.nix { inherit lib; };

      /*
        Start QEMU with U-Boot and OpenSBI.

        References:
          https://docs.u-boot.org/en/stable/board/emulation/qemu-riscv.html
          https://github.com/riscv-software-src/opensbi/blob/master/docs/platform/qemu_virt.md
      */
      mkRunQemu =
        { programName, espImage }:
        pkgs.writeShellApplication {
          name = programName;

          runtimeInputs = [
            pkgs.coreutils
            pkgs.qemu
          ];

          runtimeEnv = {
            ESP_IMAGE = espImage;
            UBOOT_IMAGE = config.packages.uboot-qemu;
            FIT_LOAD_ADDRESS = boards.toHex boards.qemu.fitLoadAddress;
          };

          text = lib.removePrefix "set -euo pipefail\n" (
            builtins.readFile ./scripts/run-qemu.sh
          );
        };
    in
    {
      packages = {
        run-qemu-debug = mkRunQemu {
          programName = "run-qemu-debug";
          espImage = config.packages.esp-image-debug;
        };

        run-qemu = mkRunQemu {
          programName = "run-qemu";
          espImage = config.packages.esp-image;
        };
      };
    };
}
