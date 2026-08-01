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

          text = builtins.readFile ./scripts/run-mangopi.sh;
        };
    in
    {
      packages = {
        # TODO: Define SD flashing (Should be the same process as for vf2?)
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
