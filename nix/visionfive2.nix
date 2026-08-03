_: {
  perSystem =
    { config, pkgs, ... }:
    let
      /*
        In UART boot mode, the VisionFive 2 BootROM receives U-Boot SPL through
        XMODEM-1K. The SPL initializes DRAM and receives `u-boot.itb` through
        YMODEM; the FIT contains OpenSBI and U-Boot proper. Once U-Boot starts,
        the runner transfers the EFI bootloader through YMODEM and launches it
        with `bootefi`.

        Sources:
        https://doc-en.rvspace.org/VisionFive2/SWTRM/VisionFive2_SW_TRM/compiling_opensbi%20-%20vf2.html
        https://docs.u-boot.org/en/stable/board/starfive/visionfive2.html
      */
      mkRunVisionFive2 =
        { programName, bootloader }:
        pkgs.writeShellApplication {
          name = programName;

          runtimeInputs = [ pkgs.tio ];

          runtimeEnv = {
            VF2_SPL_IMAGE = "${config.packages.uboot-vf2}/u-boot-spl.bin.normal.out";
            VF2_UBOOT_IMAGE = "${config.packages.uboot-vf2}/u-boot.itb";
            VF2_BOOTLOADER_IMAGE = "${bootloader}/bin/BOOTRISCV64.EFI";
            VF2_TIO_SCRIPT = ./scripts/run-vf2.lua;
          };

          text = builtins.readFile ./scripts/run-vf2.sh;
        };

      runVisionFive2Debug = mkRunVisionFive2 {
        programName = "run-vf2-debug";
        bootloader = config.packages.bootloader-debug;
      };

      runVisionFive2 = mkRunVisionFive2 {
        programName = "run-vf2";
        bootloader = config.packages.bootloader-release;
      };
    in
    {
      # TODO: Implement a way between which boot option is intended
      # TODO: formatting an SD card
      # TODO: Maybe also flash the QSPI NOR Flash memory? (WARN: Will be overwriting factory firmware)
      # TODO: Look into emulating an ESP filesystem trough uart boot (update doc at top of file)
      # TODO: Look into increasing baudrate to increase upload speeds
      packages = {
        run-vf2 = runVisionFive2;
        run-vf2-debug = runVisionFive2Debug;
      };
    };
}
