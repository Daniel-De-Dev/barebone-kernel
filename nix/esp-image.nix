_: {
  perSystem =
    { config, pkgs, ... }:
    let
      sectorSizeBytes = 512;
      partitionStartMiB = 1;

      /*
        Create a raw disk containing one FAT32 EFI System Partition.

        The first MiB is reserved for the MBR and partition alignment. The
        partition occupies the remainder of the image. Its size is increased
        in one-MiB steps until every installed payload fits.

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
          }
          ''
            bash ${./scripts/build-esp-image.sh} \
              --bootloader "${bootloader}/bin/BOOTRISCV64.EFI" \
              --output "$out" \
              --partition-start-mib ${toString partitionStartMiB} \
              --sector-size ${toString sectorSizeBytes}
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
