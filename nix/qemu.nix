_: {
  perSystem =
    {
      config,
      pkgs,
      lib,
      ...
    }:
    let
      # TODO: Move ESP generation to its own module file
      # TODO: Make patch file re-adjust reserved space dynamically so these variables remain as source of truth
      # TODO: Make the memory allocation dynamic?
      diskSizeMiB = 2; # 8
      sectorSizeBytes = 512;
      partitionStartMiB = 1;

      sectorsPerMiB = (1024 * 1024) / sectorSizeBytes;
      partitionStartSector = partitionStartMiB * sectorsPerMiB;
      partitionSizeMiB = diskSizeMiB - partitionStartMiB;
      partitionSizeSectors = partitionSizeMiB * sectorsPerMiB;

      /*
        Create a raw disk containing one FAT32 EFI System Partition.

        The first MiB is reserved for the MBR and partition alignment. The
        partition occupies the remainder of the image.

        UEFI defines \EFI\BOOT\BOOTRISCV64.EFI as the default removable-media
        boot path for 64-bit RISC-V systems.

        References:
          https://uefi.org/specs/UEFI/2.11/03_Boot_Manager.html#removable-media-boot-behavior
          https://man7.org/linux/man-pages/man8/sfdisk.8.html
      */
      mkEspImage =
        { name, bootloader }:
        pkgs.runCommand "esp-image-${name}"
          {
            nativeBuildInputs = [
              pkgs.dosfstools
              pkgs.mtools
              pkgs.util-linux
            ];
          }
          ''
            diskImage="$out/disk.img"
            partitionImage="$TMPDIR/esp-partition.img"
            espRoot="$TMPDIR/esp-root"

            mkdir -p "$out"

            # Allocate the complete raw disk image.
            truncate \
              --size=${toString diskSizeMiB}M \
              "$diskImage"

            # Create an MBR containing one EFI System Partition.
            sfdisk "$diskImage" <<EOF
            label: dos
            unit: sectors

            start=${toString partitionStartSector}, size=${toString partitionSizeSectors}, type=ef
            EOF

            truncate \
              --size=${toString partitionSizeMiB}M \
              "$partitionImage"

            mkfs.vfat \
              -F 32 \
              -n ESP \
              "$partitionImage"

            install -Dm0644 \
              "${bootloader}/bin/BOOTRISCV64.EFI" \
              "$espRoot/EFI/BOOT/BOOTRISCV64.EFI"

            mcopy \
              -i "$partitionImage" \
              -s "$espRoot/EFI" \
              ::/

            # Insert the filesystem at the partition's declared starting sector.
            dd \
              if="$partitionImage" \
              of="$diskImage" \
              bs=${toString sectorSizeBytes} \
              seek=${toString partitionStartSector} \
              conv=notrunc \
              status=none
          '';

      espImageDebug = mkEspImage {
        name = "debug";
        bootloader = config.packages.bootloader-debug;
      };

      espImageRelease = mkEspImage {
        name = "release";
        bootloader = config.packages.bootloader-release;
      };

      /*
        Start QEMU with U-boot & opensbi

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
            OPENSBI_IMAGE = config.packages.opensbi-qemu;
            UBOOT_IMAGE = config.packages.uboot-qemu;
          };

          text = lib.removePrefix "set -euo pipefail\n" (
            builtins.readFile ./scripts/run-qemu.sh
          );
        };

      runQemuDebug = mkRunQemu {
        programName = "run-qemu-debug";
        espImage = espImageDebug;
      };

      runQemu = mkRunQemu {
        programName = "run-qemu";
        espImage = espImageRelease;
      };
    in
    {
      packages = {
        esp-image-debug = espImageDebug;
        esp-image = espImageRelease;

        run-qemu-debug = runQemuDebug;
        run-qemu = runQemu;
      };
    };
}
