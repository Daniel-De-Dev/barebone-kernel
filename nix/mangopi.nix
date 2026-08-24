{ inputs, ... }: {
  perSystem =
    {
      config,
      pkgs,
      boards,
      lib,
      ...
    }:
    let
      board = boards.mangopi;

      opensbiLib = import ./opensbi.nix { inherit inputs lib pkgs; };

      mangoPiDtbName = "mangopi-mq-pro.dtb";
      # TODO: Look into reserving the memory region for DTB if OpenSBI
      # doesn't already dynamically (also for vf2)
      mangoPiDtsiName = "barebone-memory.dtsi";

      /*
        Build the hardware DTB passed to OpenSBI and the kernel.

        The MangoPi MQ Pro hardware description is compiled directly from
        `arch/riscv/dts/sun20i-d1-mangopi-mq-pro.dts`.

        The upstream board DTS does not contain a DRAM memory node. Since the FEL
        path initializes DRAM before entering OpenSBI, the known DRAM region is
        injected from the board memory map before compiling the hardware DTB.

        Source:
        https://github.com/smaeul/u-boot/blob/2e89b706f5c956a70c989cd31665f1429e9a0b48/arch/riscv/dts/sun20i-d1-mangopi-mq-pro.dts
      */
      mangoPiDtb = pkgs.stdenv.mkDerivation {
        pname = "mangopi-mq-pro-dtb";
        version = inputs.src-uboot-d1.shortRev or "dirty";
        src = inputs.src-uboot-d1;

        postPatch = ''
          patchShebangs scripts tools

          substituteInPlace \
            arch/riscv/dts/sun20i-d1-mangopi-mq-pro.dts \
            --replace-fail \
              '#include "sun20i-common-regulators.dtsi"' \
              $'#include "sun20i-common-regulators.dtsi"\n#include "${mangoPiDtsiName}"'

          install -Dm0644 \
            ${./templates/mangopi-memory.dtsi.in} \
            arch/riscv/dts/${mangoPiDtsiName}

          substituteInPlace arch/riscv/dts/${mangoPiDtsiName} \
            --replace-fail '@dramUnitAddress@' '${lib.toHexString board.dramBase}' \
            --replace-fail '@dramBase@' '${boards.toHex board.dramBase}' \
            --replace-fail '@dramSize@' '${boards.toHex board.dramSize}'
        '';

        dontConfigure = true;

        buildPhase = ''
          runHook preBuild

          ${pkgs.stdenv.cc}/bin/cpp \
            -nostdinc \
            -I include \
            -I arch/riscv/dts \
            -undef \
            -D__DTS__ \
            -x assembler-with-cpp \
            -o mangopi-mq-pro.dts.preprocessed \
            arch/riscv/dts/sun20i-d1-mangopi-mq-pro.dts

          ${lib.getExe pkgs.dtc} \
            -I dts \
            -O dtb \
            -o ${mangoPiDtbName} \
            mangopi-mq-pro.dts.preprocessed

          runHook postBuild
        '';

        installPhase = ''
          runHook preInstall

          install -Dm0644 \
            ${mangoPiDtbName} \
            "$out/${mangoPiDtbName}"

          runHook postInstall
        '';
      };

      opensbiMangoPi = opensbiLib.mkJump {
        name = "mangopi-fel";
        textStart = board.opensbiAddress;
        jumpAddress = board.kernelAddress;
        fdtPath = "${mangoPiDtb}/${mangoPiDtbName}";
        inherit (board) fdtAddress;
      };

      /*
        Boot the MangoPi MQ Pro through FEL.

        xfel initializes the D1 DRAM controller, uploads the kernel and
        OpenSBI to their fixed addresses, then starts OpenSBI.

        OpenSBI uses FW_JUMP because the next-stage kernel is already
        present at a fixed address. The FEL path does not supply OpenSBI with
        a DTB, so the firmware embeds the generated board DTB and passes it
        to the kernel at the configured FDT address.

        No U-Boot SPL or U-Boot proper runs.

        Sources:
        https://xfel.xboot.org/en/command/ddr
        https://xfel.xboot.org/en/command/write
        https://xfel.xboot.org/en/command/exec
        https://github.com/riscv-software-src/opensbi/blob/c0f87f10d1bfb9e72a84ddfafb5604ee1bfe9d04/docs/firmware/fw_jump.md
      */
      mkRunMangoPi =
        { programName, kernel }:
        pkgs.writeShellApplication {
          name = programName;

          runtimeInputs = [
            pkgs.coreutils
            pkgs.tio
            pkgs.xfel
          ];

          runtimeEnv = {
            MANGOPI_OPENSBI_IMAGE = "${opensbiMangoPi}/fw_jump.bin";
            MANGOPI_OPENSBI_ADDRESS = boards.toHex board.opensbiAddress;
            MANGOPI_KERNEL_IMAGE = "${kernel}/bin/kernel.bin";
            MANGOPI_KERNEL_ADDRESS = boards.toHex board.kernelAddress;
            MANGOPI_BAUDRATE = toString board.baudrate;
          };

          text = ''
            # shellcheck source=/dev/null
            source ${./scripts/common.sh}
            ${builtins.readFile ./scripts/run-mangopi.sh}
          '';
        };
    in
    {
      packages = {
        # TODO: Define SD flashing
        mangopi-debug = mkRunMangoPi {
          programName = "run-mangopi-debug";
          kernel = config.packages.kernel-mangopi-debug;
        };

        mangopi = mkRunMangoPi {
          programName = "run-mangopi";
          kernel = config.packages.kernel-mangopi;
        };

        mangopi-dtb = mangoPiDtb;
      };
    };
}
