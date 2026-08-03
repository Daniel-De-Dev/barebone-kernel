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
          # TODO: Switch to using env vars as there is no reason for it to take arguments
          ''
            bash ${./scripts/build-esp-image.sh} \
              --bootloader "${bootloader}/bin/BOOTRISCV64.EFI" \
              --output "$out" \
              --partition-start-mib ${toString partitionStartMiB} \
              --sector-size ${toString sectorSizeBytes} \
              --disk-size ${toString boards.ramDiskSize}
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
