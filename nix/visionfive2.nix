_: {
  perSystem =
    { config, pkgs, ... }:
    let
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
      # TODO: Implement distinct choice to boot trough usart and also flashing
      # formatting an SD card
      packages = {
        run-vf2 = runVisionFive2;
        run-vf2-debug = runVisionFive2Debug;
      };
    };
}
