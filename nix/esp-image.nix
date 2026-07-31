_: {
  perSystem =
    { config, pkgs, ... }:
    let
      # TODO: Determine the disk size from the installed payloads.
      # TODO: Ensure the disk size aligns with what mangopi assumes forexample
      diskSizeMiB = 2;
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

        Sources:
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

            truncate \
              --size=${toString diskSizeMiB}M \
              "$diskImage"

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

            dd \
              if="$partitionImage" \
              of="$diskImage" \
              bs=${toString sectorSizeBytes} \
              seek=${toString partitionStartSector} \
              conv=notrunc \
              status=none
          '';
    in
    {
      packages = {
        esp-image-debug = mkEspImage {
          name = "debug";
          bootloader = config.packages.bootloader-debug;
        };

        esp-image = mkEspImage {
          name = "release";
          bootloader = config.packages.bootloader-release;
        };
      };
    };
}
