_: {
  perSystem =
    {
      config,
      pkgs,
      lib,
      ...
    }:
    let
      boards = import ./boards.nix { inherit lib; };

      # TODO: Make patch file re-adjust reserved space dynamically so these variables remain as source of truth
      ramDiskCapacityBytes = 64 * 1024 * 1024;

      mkRunMangoPi =
        { programName, espImage }:
        pkgs.writeShellApplication {
          name = programName;

          runtimeInputs = [
            pkgs.coreutils
            pkgs.tio
            pkgs.xfel
          ];

          runtimeEnv = {
            MANGOPI_OPENSBI_IMAGE = "${config.packages.opensbi-mangopi-fel}/fw_jump.bin";
            MANGOPI_OPENSBI_ADDRESS = boards.mangopi.opensbiAddress;
            MANGOPI_UBOOT_IMAGE = "${config.packages.uboot-mangopi}/u-boot.bin";
            MANGOPI_UBOOT_ADDRESS = boards.mangopi.ubootAddress;
            MANGOPI_DISK_IMAGE = "${espImage}/disk.img";
            MANGOPI_RAM_DISK_ADDRESS = boards.mangopi.ramDiskAddress;
            MANGOPI_RAM_DISK_CAPACITY_BYTES = toString ramDiskCapacityBytes;
            MANGOPI_EFI_LOAD_ADDRESS = boards.mangopi.efiLoadAddress;
            MANGOPI_TIO_SCRIPT = ./scripts/run-mangopi.lua;
          };

          text = builtins.readFile ./scripts/run-mangopi.sh;
        };
    in
    {
      packages = {
        # TODO: Define SD flashing (Should be the same process as for vf2?)
        # TODO: Also flash memory flashing (if it has)
        run-mangopi-debug = mkRunMangoPi {
          programName = "run-mangopi-debug";
          espImage = config.packages.esp-image-debug;
        };

        run-mangopi = mkRunMangoPi {
          programName = "run-mangopi";
          espImage = config.packages.esp-image;
        };
      };
    };
}
