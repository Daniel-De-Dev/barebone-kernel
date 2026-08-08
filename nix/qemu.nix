{ inputs, ... }: {
  perSystem =
    {
      config,
      pkgs,
      lib,
      boards,
      ...
    }:
    let
      opensbiLib = import ./opensbi.nix { inherit inputs lib pkgs; };

      /*
        QEMU loads OpenSBI as machine firmware and places the bootloader at its
        fixed load address in DRAM. OpenSBI FW_JUMP then enters the bootloader
        at that address in S-mode.

        QEMU's `virt` machine provides its generated FDT to OpenSBI.

        Sources:
        https://www.qemu.org/docs/master/system/riscv/virt.html#hardware-configuration-information
      */
      opensbiQemu = opensbiLib.mkJump {
        name = "jump-qemu";
        textStart = boards.qemu.opensbiAddress;
        jumpAddress = boards.qemu.bootloaderAddress;
      };

      /*
        Run the bootloader on QEMU's RISC-V `virt` machine.

        OpenSBI FW_JUMP is installed as the machine firmware with `-bios`.
        QEMU's generic loader places the raw bootloader image at the address
        for which it was linked. The loader does not change the CPU entry
        point; execution begins in OpenSBI, which later jumps to the
        bootloader.

        Sources:
        https://www.qemu.org/docs/master/system/riscv/virt.html
        https://www.qemu.org/docs/master/system/generic-loader.html
      */
      mkRunQemu =
        {
          programName,
          bootloader,
          opensbi,
        }:
        pkgs.writeShellApplication {
          name = programName;

          runtimeInputs = [
            pkgs.coreutils
            pkgs.qemu
          ];

          runtimeEnv = {
            OPENSBI_IMAGE = "${opensbi}/fw_jump.bin";
            BOOTLOADER_IMAGE = "${bootloader}/bin/bootloader.bin";
            BOOTLOADER_ADDRESS = boards.toHex bootloader.loadAddress;
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
          bootloader = config.packages.bootloader-qemu-debug;
          opensbi = opensbiQemu;
        };

        run-qemu = mkRunQemu {
          programName = "run-qemu";
          bootloader = config.packages.bootloader-qemu;
          opensbi = opensbiQemu;
        };
      };
    };
}
