_: {
  perSystem =
    {
      config,
      pkgs,
      lib,
      ...
    }:
    let
      sectorSizeBytes = 512;
      partitionStartMiB = 1;

      boards = import ./boards.nix { inherit lib; };

      /*
        Create a raw disk containing one FAT32 EFI System Partition.

        The first MiB is reserved for the MBR and partition alignment. The
        partition occupies the remainder of the image. Its tries to fit in the
        specified size, otherwise it fails.

        `disk-size-bytes` records the final raw image size for consumers which
        must reserve an equally sized memory region.

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

            BOOTLOADER_IMAGE = "${bootloader}/bin/BOOTRISCV64.EFI";
            PARTITION_START_MIB = toString partitionStartMiB;
            SECTOR_SIZE_BYTES = toString sectorSizeBytes;
            DISK_SIZE_BYTES = toString boards.ramDiskSize;

          }
          ''
            source ${./scripts/common.sh}
            ${builtins.readFile ./scripts/build-esp-image.sh}
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
