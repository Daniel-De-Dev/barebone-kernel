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

      /*
        In UART boot mode, the VisionFive 2 BootROM receives U-Boot SPL through
        XMODEM-1K. The SPL initializes DRAM and receives `u-boot.itb` through
        YMODEM; the FIT contains OpenSBI and U-Boot proper. Once U-Boot starts,
        the runner transfers a disk image into ram which u-boot uses as a disk.

        Sources:
        https://doc-en.rvspace.org/VisionFive2/SWTRM/VisionFive2_SW_TRM/compiling_opensbi%20-%20vf2.html
        https://docs.u-boot.org/en/stable/board/starfive/visionfive2.html
      */
      mkRunVisionFive2 =
        { programName, espImage }:
        pkgs.writeShellApplication {
          name = programName;

          runtimeInputs = [ pkgs.tio ];

          runtimeEnv = {
            VF2_SPL_IMAGE = "${config.packages.uboot-vf2}/u-boot-spl.bin.normal.out";
            VF2_UBOOT_IMAGE = "${config.packages.uboot-vf2}/u-boot.itb";
            VF2_DISK_IMAGE = "${espImage}/disk.img";
            VF2_DISK_SIZE_FILE = "${espImage}/disk-size-bytes";
            VF2_RAM_DISK_ADDRESS = boards.toHex boards.visionfive2.ramDiskAddress;
            VF2_EFI_LOAD_ADDRESS = boards.toHex boards.visionfive2.efiLoadAddress;
            VF2_BAUDRATE_BOOTROM = toString boards.visionfive2.baudrateBootROM;
            VF2_BAUDRATE = toString boards.visionfive2.baudrate;
            VF2_SPL_TIO_SCRIPT = ./scripts/run-vf2-spl.lua;
            VF2_FIT_TIO_SCRIPT = ./scripts/run-vf2-fit.lua;
          };

          text = ''
            # shellcheck source=/dev/null
            source ${./scripts/common.sh}
            ${builtins.readFile ./scripts/run-vf2.sh}
          '';
        };

      runVisionFive2Debug = mkRunVisionFive2 {
        programName = "run-vf2-debug";
        espImage = config.packages.esp-image-debug;
      };

      runVisionFive2 = mkRunVisionFive2 {
        programName = "run-vf2";
        espImage = config.packages.esp-image;
      };
    in
    {
      # TODO: Implement a way between which boot option is intended
      # TODO: formatting an SD card
      # TODO: Maybe also flash the QSPI NOR Flash memory? (WARN: Will be overwriting factory firmware)
      packages = {
        run-vf2 = runVisionFive2;
        run-vf2-debug = runVisionFive2Debug;
      };
    };
}
