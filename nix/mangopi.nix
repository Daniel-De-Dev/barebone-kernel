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
        The MangoPi MQ Pro FEL path starts after the D1 BootROM enters FEL mode.
        xfel initializes DRAM, uploads U-Boot proper, an ESP disk image, and
        OpenSBI FW_JUMP, then starts OpenSBI directly. No U-Boot SPL runs.

        `xfel exec` does not provide an FDT address in a1. The firmware build
        therefore embeds U-Boot's DTB in OpenSBI, which relocates it to the
        configured staging address and passes it to U-Boot proper. Because no
        SPL prepares the DTB, the U-Boot build adds the DRAM description. It
        also reserves the uploaded ESP range so U-Boot does not overwrite that
        memory.

        U-Boot exposes the ESP through blkmap, loads `BOOTRISCV64.EFI` from its
        FAT partition, and launches it with `bootefi`.

        Sources:
        https://xfel.xboot.org/en/command/ddr
        https://xfel.xboot.org/en/command/exec
        https://docs.u-boot.org/en/stable/usage/blkmap.html
        https://github.com/riscv-software-src/opensbi/blob/c0f87f10d1bfb9e72a84ddfafb5604ee1bfe9d04/docs/firmware/fw_jump.md
      */

      boards = import ./boards.nix { inherit lib; };

      mkRunMangoPi =
        {
          programName,
          espImage,
          opensbi,
          uboot,
        }:
        pkgs.writeShellApplication {
          name = programName;

          runtimeInputs = [
            pkgs.coreutils
            pkgs.tio
            pkgs.xfel
          ];

          runtimeEnv = {
            MANGOPI_OPENSBI_IMAGE = "${opensbi}/fw_jump.bin";
            MANGOPI_OPENSBI_ADDRESS = boards.toHex boards.mangopi.opensbiAddress;
            MANGOPI_UBOOT_IMAGE = "${uboot}/u-boot.bin";
            MANGOPI_UBOOT_ADDRESS = boards.toHex boards.mangopi.ubootAddress;
            MANGOPI_DISK_IMAGE = "${espImage}/disk.img";
            MANGOPI_DISK_SIZE_FILE = "${espImage}/disk-size-bytes";
            MANGOPI_RAM_DISK_ADDRESS = boards.toHex boards.mangopi.ramDiskAddress;
            MANGOPI_EFI_LOAD_ADDRESS = boards.toHex boards.mangopi.efiLoadAddress;
            MANGOPI_TIO_SCRIPT = ./scripts/run-mangopi.lua;
          };

          text = ''
            # shellcheck source=/dev/null
            source ${./scripts/common.sh}
            ${builtins.readFile ./scripts/run-mangopi.sh}
          '';
        };
    in
    {
      packages = {
        # TODO: Define SD flashing (Should be the same process as for vf2?)
        # TODO: Look into speeding up upload speeds for FEL
        run-mangopi-debug = mkRunMangoPi {
          programName = "run-mangopi-debug";
          espImage = config.packages.esp-image-debug;
          opensbi = config.packages.opensbi-mangopi-fel-debug;
          uboot = config.packages.uboot-mangopi-debug;
        };

        run-mangopi = mkRunMangoPi {
          programName = "run-mangopi";
          espImage = config.packages.esp-image;
          opensbi = config.packages.opensbi-mangopi-fel;
          uboot = config.packages.uboot-mangopi;
        };
      };
    };
}
