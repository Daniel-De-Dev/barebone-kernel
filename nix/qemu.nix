_: {
  perSystem =
    {
      config,
      pkgs,
      lib,
      ...
    }:
    let
      /*
        QEMU loads U-Boot SPL as the `virt` machine firmware and places the FIT
        image at the address expected by the SPL. The FIT contains OpenSBI and
        U-Boot proper. U-Boot then discovers the attached ESP as a VirtIO block
        device and launches its default RISC-V EFI application.

        Sources:
        https://www.qemu.org/docs/master/system/riscv/virt.html#running-u-boot
        https://docs.u-boot.org/en/stable/board/emulation/qemu-riscv.html
        https://github.com/riscv-software-src/opensbi/blob/master/docs/platform/qemu_virt.md
      */

      boards = import ./boards.nix { inherit lib; };

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

          text = ''
            # shellcheck source=/dev/null
            source ${./scripts/common.sh}
            ${lib.removePrefix "set -euo pipefail\n" (
              builtins.readFile ./scripts/run-qemu.sh
            )}
          '';
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
