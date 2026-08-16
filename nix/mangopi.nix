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
      riscvPkgs = pkgs.pkgsCross.riscv64;
      crossCompile = riscvPkgs.stdenv.cc.targetPrefix;

      board = boards.mangopi;

      opensbiLib = import ./opensbi.nix { inherit inputs lib pkgs; };

      mangoPiDtbName = "mangopi-mq-pro.dtb";
      # TODO: Look into reserving the memory region for DTB if OpenSBI
      # doesn't already dynamically
      mangoPiDtsiName = "barebone-memory.dtsi";

      /*
        Build the DTB used by the MangoPi MQ Pro FEL boot path.

        The board DTS does not contain a DRAM memory node. Since the FEL path
        enters OpenSBI without an SPL preparing the DTB, inject the known DRAM
        region from the board memory map before building it.

        U-Boot proper is not built or used; its D1 source tree is only used to
        build the board DTB.

        Source:
        https://github.com/smaeul/u-boot/blob/2e89b706f5c956a70c989cd31665f1429e9a0b48/arch/riscv/dts/sun20i-d1-mangopi-mq-pro.dts
      */
      mangoPiDtb = riscvPkgs.stdenv.mkDerivation {
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
            --replace-fail '@dramUnitAddress@' '${lib.toHexString boards.mangopi.dramBase}' \
            --replace-fail '@dramBase@' '${boards.toHex boards.mangopi.dramBase}' \
            --replace-fail '@dramSize@' '${boards.toHex boards.mangopi.dramSize}'
        '';

        nativeBuildInputs = with pkgs; [
          bc
          bison
          flex
          (python3.withPackages (ps: [
            ps.libfdt
            ps.setuptools_80
          ]))
          stdenv.cc
          perl
        ];

        buildInputs = with pkgs; [
          openssl
          gnutls
        ];

        enableParallelBuilding = true;

        env = {
          ARCH = "riscv";
          CROSS_COMPILE = crossCompile;
          DTC = lib.getExe pkgs.dtc;
        };

        buildFlags = [ "u-boot.dtb" ];

        configurePhase = ''
          runHook preConfigure

          make -j"$NIX_BUILD_CORES" mangopi_mq_pro_defconfig

          runHook postConfigure
        '';

        installPhase = ''
          runHook preInstall

          install -Dm0644 \
            u-boot.dtb \
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
      };
    };
}
