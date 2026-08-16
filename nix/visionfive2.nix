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

      opensbiLib = import ./opensbi.nix { inherit inputs lib pkgs; };

      visionFive2Dtb = "jh7110-starfive-visionfive-2-v1.3b.dtb";

      opensbiVisionFive2 = opensbiLib.mkDynamic {
        name = "visionfive2";
        textStart = boards.visionfive2.opensbiAddress;
      };

      /*
        Build the U-Boot SPL used as the VisionFive 2 first-stage kernel.

        SPL is used only for the board's early hardware and DRAM
        initialization and for loading the next-stage FIT image. U-Boot proper
        is not used.

        `starfive_visionfive2_defconfig` produces the StarFive-wrapped
        `u-boot-spl.bin.normal.out` image accepted by the board's BootROM.

        The v1.3B DTB is also installed for inclusion in the project's custom
        FIT image.
      */
      visionFive2Spl = riscvPkgs.stdenv.mkDerivation {
        pname = "visionfive2-spl";
        version = inputs.src-uboot.shortRev or "dirty";
        src = inputs.src-uboot;

        postPatch = ''
          patchShebangs scripts tools
        '';

        nativeBuildInputs = with pkgs; [
          bison
          flex
          (python3.withPackages (ps: [ ps.libfdt ]))
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
          OPENSBI = "${opensbiVisionFive2}/fw_dynamic.bin";
          DTC = lib.getExe pkgs.dtc;
        };

        configurePhase = ''
          runHook preConfigure

          make -j$NIX_BUILD_CORES starfive_visionfive2_defconfig

          ./scripts/config \
            --set-val CONFIG_BAUDRATE \
            ${toString boards.visionfive2.baudrate}

          runHook postConfigure
        '';

        installPhase = ''
          runHook preInstall

          install -Dm0644 \
            spl/u-boot-spl.bin.normal.out \
            "$out/u-boot-spl.bin.normal.out"

          src="$(find . -type f -name "${visionFive2Dtb}" -print -quit)"

          if [ -z "$src" ]; then
            echo "Could not find built DTB: ${visionFive2Dtb}" >&2
            exit 1
          fi

          install -Dm0644 "$src" "$out/dtb/${visionFive2Dtb}"

          runHook postInstall
        '';
      };

      /*
        Build the FIT consumed by the VisionFive 2 U-Boot SPL.

        The image contains OpenSBI FW_DYNAMIC, the kernel, and
        the board DTB.
      */
      mkVisionFive2Fit =
        { name, kernel }:
        pkgs.stdenv.mkDerivation {
          pname = "visionfive2-fit-${name}";
          version = "0.1.0";

          dontUnpack = true;

          nativeBuildInputs = [
            pkgs.ubootTools
            pkgs.dtc
          ];

          buildPhase = ''
            runHook preBuild

            cp ${./templates/vf2-fit.its.in} fit.its

            substituteInPlace fit.its \
              --replace-fail \
                '@opensbiAddress@' \
                '${boards.toHex boards.visionfive2.opensbiAddress}' \
              --replace-fail \
                '@kernelAddress@' \
                '${boards.toHex kernel.loadAddress}' \
              --replace-fail \
                '@fdtAddress@' \
                '${boards.toHex boards.visionfive2.fdtAddress}'

            ln -s ${opensbiVisionFive2}/fw_dynamic.bin fw_dynamic.bin
            ln -s ${kernel}/bin/kernel.bin kernel.bin
            ln -s \
              ${visionFive2Spl}/dtb/${visionFive2Dtb} \
              vf2-v1.3b.dtb

            mkimage -f fit.its fit.itb

            runHook postBuild
          '';

          installPhase = ''
            install -Dm0644 fit.itb $out/fit.itb
          '';
        };

      /*
        Boot the VisionFive 2 through its UART recovery path.

        The BootROM receives U-Boot SPL. SPL then performs the board's early
        hardware and DRAM initialization, then receives the custom FIT image.

        The FIT contains OpenSBI FW_DYNAMIC, the kernel, and the board DTB.

        Sources:
        https://github.com/u-boot/u-boot/blob/baa64b2f892890f00a377eac4a3e685472bb56b5/common/spl/spl_opensbi.c
        https://github.com/u-boot/u-boot/blob/baa64b2f892890f00a377eac4a3e685472bb56b5/board/starfive/visionfive2/spl.c
      */
      mkRunVisionFive2 =
        { programName, fitImage }:
        pkgs.writeShellApplication {
          name = programName;

          runtimeInputs = [ pkgs.tio ];

          runtimeEnv = {
            VF2_SPL_IMAGE = "${visionFive2Spl}/u-boot-spl.bin.normal.out";
            VF2_FIT_IMAGE = "${fitImage}/fit.itb";
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
        fitImage = mkVisionFive2Fit {
          name = "debug";
          kernel = config.packages.kernel-vf2-debug;
        };
      };

      runVisionFive2 = mkRunVisionFive2 {
        programName = "run-vf2";
        fitImage = mkVisionFive2Fit {
          name = "release";
          kernel = config.packages.kernel-vf2;
        };
      };
    in
    {
      # TODO: Implement a way between which boot option is intended
      # TODO: formatting an SD card
      # TODO: Maybe also flash the QSPI NOR Flash memory? (WARN: Will be overwriting factory firmware)
      packages = {
        vf2 = runVisionFive2;
        vf2-debug = runVisionFive2Debug;
      };
    };
}
