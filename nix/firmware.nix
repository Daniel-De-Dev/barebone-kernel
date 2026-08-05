{ inputs, ... }: {
  perSystem =
    { lib, pkgs, ... }:
    let
      riscvPkgs = pkgs.pkgsCross.riscv64;
      crossCompile = riscvPkgs.stdenv.cc.targetPrefix;

      boards = import ./boards.nix { inherit lib; };

      /*
        Build an OpenSBI FW_DYNAMIC firmware image for RISC-V.

        `name` identifies the target and forms part of the package name.

        `textStart` sets OpenSBI's FW_TEXT_START.

        The package installs `fw_dynamic.bin`.

        # TODO: Update permalinks (github.com/...) to align with flake.lock (for all links, update flake before)
        Sources:
        https://github.com/riscv-software-src/opensbi/blob/c0f87f10d1bfb9e72a84ddfafb5604ee1bfe9d04/docs/firmware/fw_dynamic.md
      */
      mkOpenSBIDynamic =
        { name, textStart }:
        riscvPkgs.stdenv.mkDerivation {
          pname = "opensbi-dynamic-${name}";
          version = inputs.src-opensbi.shortRev or "dirty";
          src = inputs.src-opensbi;

          nativeBuildInputs = [ pkgs.python3 ];

          postPatch = ''
            patchShebangs scripts
          '';

          enableParallelBuilding = true;

          env = {
            ARCH = "riscv";
            CROSS_COMPILE = crossCompile;
            PLATFORM = "generic";
            FW_TEXT_START = textStart;
          };

          installPhase = ''
            runHook preInstall

            install -Dm0644 \
              build/platform/generic/firmware/fw_dynamic.bin \
              "$out/fw_dynamic.bin"

            runHook postInstall
          '';
        };

      opensbiQemu = mkOpenSBIDynamic {
        name = "qemu";
        textStart = boards.toHex boards.qemu.opensbiAddress;
      };

      opensbiVisionFive2 = mkOpenSBIDynamic {
        name = "visionfive2";
        textStart = boards.toHex boards.visionfive2.opensbiAddress;
      };

      /*
        Build U-Boot for a RISC-V target using OpenSBI FW_DYNAMIC.

        `name` identifies the target and forms part of the package name.

        `defconfig` is the U-Boot default configuration target passed to
        `make`. It selects the board and its initial U-Boot configuration.

        `opensbi` is a package providing `fw_dynamic.bin`. Its path is
        exposed to the U-Boot build through `OPENSBI`, allowing configurations
        that generate `u-boot.itb` to include OpenSBI and U-Boot proper in the
        FIT image.

        The package installs `u-boot.bin` and any supported FIT or SPL artifacts
        emitted by the selected configuration.
      */
      mkUBoot =
        {
          name,
          defconfig,
          opensbi,
        }:
        riscvPkgs.stdenv.mkDerivation {
          pname = "u-boot-${name}";
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
            DTC = lib.getExe pkgs.dtc;
            OPENSBI = "${opensbi}/fw_dynamic.bin";
          };

          configurePhase = ''
            runHook preConfigure

            make -j$NIX_BUILD_CORES ${defconfig}

            runHook postConfigure
          '';

          installPhase = ''
            runHook preInstall

            install -Dm0644 u-boot.bin "$out/u-boot.bin"

            # Install optional artifacts emitted by the selected defconfig.
            if [ -f u-boot.itb ]; then
              install -Dm0644 u-boot.itb "$out/u-boot.itb"
            fi

            if [ -f spl/u-boot-spl ]; then
              install -Dm0644 spl/u-boot-spl "$out/u-boot-spl"
            fi

            if [ -f spl/u-boot-spl.bin.normal.out ]; then
              install -Dm0644 \
                spl/u-boot-spl.bin.normal.out \
                "$out/u-boot-spl.bin.normal.out"
            fi

            runHook postInstall
          '';
        };

      /*
        Build U-Boot proper and its DTB for the MangoPi MQ Pro FEL runner.

        `name` identifies the build variant and forms part of the package name.

        The build enables CONFIG_BLKMAP and installs `u-boot.bin` and
        `u-boot.dtb`.
      */
      mkUBootMangoPi =
        { name }:
        let
          inherit (boards) ramDiskSize;
          inherit (boards.mangopi)
            dramStart
            dramSize
            ramDiskAddress
            fdtAddress
            efiLoadAddress
            ubootAddress
            ;

          dramEnd = dramStart + dramSize;
          ramDiskEnd = ramDiskAddress + ramDiskSize;

          mib = 1024 * 1024;
        in
        # TODO: Consider more exstensive asserts and reconsider asserts
        # across nix files and scripts when it comes being confident in
        # getting good feedback and being sure assumptions are meet.
        # Maybe not to gurantee absolute correct state, but just have
        # general bound checks and other important sanity chekcs to avoid
        # headaches when accidentally breaking assumptiosn when refactoring
        # Idea: define simialrly to ram disk hardocded space allocated and
        # and assert sizes to fit within the allocated size...
        assert lib.assertMsg (
          ramDiskAddress >= dramStart
        ) "RAM disk address falls before DRAM start";
        assert lib.assertMsg (
          ramDiskEnd <= dramEnd
        ) "ESP reservation extends beyond MangoPi DRAM";
        assert lib.assertMsg
          # WARN: size isint taken into account
          (fdtAddress >= dramStart && fdtAddress < dramEnd)
          "FDT staging address is outside DRAM";
        assert lib.assertMsg
          # WARN: size isint taken into account
          (efiLoadAddress >= dramStart && efiLoadAddress < dramEnd)
          "EFI load address is outside DRAM";
        assert lib.assertMsg
          # WARN: size isint taken into account
          (ubootAddress >= dramStart && ubootAddress < dramEnd)
          "U-Boot staging address is outside DRAM";
        assert lib.assertMsg (
          lib.mod ramDiskSize mib == 0
        ) "RAM disk size must be a multiple of 1 MiB";

        riscvPkgs.stdenv.mkDerivation {
          pname = "u-boot-mangopi-fel-${name}";
          version = inputs.src-uboot-d1.shortRev or "dirty";
          src = inputs.src-uboot-d1;

          postPatch = ''
            patchShebangs scripts tools

            substituteInPlace \
              arch/riscv/dts/sun20i-d1-mangopi-mq-pro.dts \
              --replace-fail \
                '#include "sun20i-common-regulators.dtsi"' \
                $'#include "sun20i-common-regulators.dtsi"\n#include "barebone-fel-memory.dtsi"'

            install -Dm0644 \
              ${./dts/mangopi-fel-memory.dtsi.in} \
              arch/riscv/dts/barebone-fel-memory.dtsi

            substituteInPlace arch/riscv/dts/barebone-fel-memory.dtsi \
              --replace-fail '@dramUnitAddress@' '${lib.toHexString dramStart}' \
              --replace-fail '@dramStart@' '${boards.toHex dramStart}' \
              --replace-fail '@dramSize@' '${boards.toHex dramSize}' \
              --replace-fail '@ramDiskUnitAddress@' '${lib.toHexString ramDiskAddress}' \
              --replace-fail '@ramDiskAddress@' '${boards.toHex ramDiskAddress}' \
              --replace-fail '@ramDiskSize@' '${boards.toHex ramDiskSize}'
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

          buildFlags = [
            "u-boot.bin"
            "u-boot.dtb"
          ];

          configurePhase = ''
            runHook preConfigure

            make -j"$NIX_BUILD_CORES" mangopi_mq_pro_defconfig

            ./scripts/config --enable CONFIG_BLKMAP

            runHook postConfigure
          '';

          installPhase = ''
            runHook preInstall

            install -Dm0644 \
              u-boot.bin \
              "$out/u-boot.bin"

            install -Dm0644 \
              u-boot.dtb \
              "$out/u-boot.dtb"

            runHook postInstall
          '';
        };

      /*
        Build OpenSBI FW_JUMP for the MangoPi MQ Pro FEL runner.

        `name` identifies the build variant and forms part of the package name.

        `uboot` provides the DTB embedded through `FW_FDT_PATH`. Firmware and
        next-stage addresses come from the MangoPi board map.

        The package installs `fw_jump.bin`.
      */
      mkOpenSBIMangoPiFel =
        { name, uboot }:
        riscvPkgs.stdenv.mkDerivation {
          pname = "opensbi-jump-mangopi-fel-${name}";
          version = inputs.src-opensbi.shortRev or "dirty";
          src = inputs.src-opensbi;

          nativeBuildInputs = [ pkgs.python3 ];

          postPatch = ''
            patchShebangs scripts
          '';

          enableParallelBuilding = true;

          env = {
            ARCH = "riscv";
            CROSS_COMPILE = crossCompile;
            PLATFORM = "generic";
            FW_JUMP = "y";
            FW_TEXT_START = boards.toHex boards.mangopi.opensbiAddress;
            FW_JUMP_ADDR = boards.toHex boards.mangopi.ubootAddress;
            FW_FDT_PATH = "${uboot}/u-boot.dtb";
            FW_JUMP_FDT_ADDR = boards.toHex boards.mangopi.fdtAddress;
          };

          installPhase = ''
            runHook preInstall

            install -Dm0644 \
              build/platform/generic/firmware/fw_jump.bin \
              "$out/fw_jump.bin"

            runHook postInstall
          '';
        };

      ubootMangoPiDebug = mkUBootMangoPi { name = "debug"; };

      ubootMangoPi = mkUBootMangoPi { name = "release"; };

      opensbiMangoPiFelDebug = mkOpenSBIMangoPiFel {
        name = "debug";
        uboot = ubootMangoPiDebug;
      };

      opensbiMangoPiFel = mkOpenSBIMangoPiFel {
        name = "release";
        uboot = ubootMangoPi;
      };
    in
    {
      packages = {
        opensbi-qemu = opensbiQemu;
        opensbi-vf2 = opensbiVisionFive2;
        opensbi-mangopi-fel-debug = opensbiMangoPiFelDebug;
        opensbi-mangopi-fel = opensbiMangoPiFel;

        uboot-qemu = mkUBoot {
          name = "qemu";
          defconfig = "qemu-riscv64_spl_defconfig";
          opensbi = opensbiQemu;
        };

        # Due to uploading the disk image to ram; Override to compile memory
        # reservation and enable blkmap feature.
        uboot-vf2 =
          (mkUBoot {
            name = "visionfive2";
            defconfig = "starfive_visionfive2_defconfig";
            opensbi = opensbiVisionFive2;
          }).overrideAttrs
            (
              _finalAttrs: previousAttrs: {
                postPatch = (previousAttrs.postPatch or "") + ''
                  substituteInPlace \
                    arch/riscv/dts/starfive-visionfive2-u-boot.dtsi \
                    --replace-fail \
                      '#include "starfive-visionfive2-binman.dtsi"' \
                      $'#include "starfive-visionfive2-binman.dtsi"\n#include "barebone-vf2-memory.dtsi"'

                  install -Dm0644 \
                    ${./dts/vf2-ramdisk-memory.dtsi.in} \
                    arch/riscv/dts/barebone-vf2-memory.dtsi

                  substituteInPlace arch/riscv/dts/barebone-vf2-memory.dtsi \
                    --replace-fail '@ramDiskUnitAddress@' '${lib.toHexString boards.visionfive2.ramDiskAddress}' \
                    --replace-fail '@ramDiskAddress@' '${boards.toHex boards.visionfive2.ramDiskAddress}' \
                    --replace-fail '@ramDiskSize@' '${boards.toHex boards.ramDiskSize}'
                '';

                configurePhase = (previousAttrs.configurePhase or "") + ''
                  ./scripts/config --enable CONFIG_BLKMAP
                '';
              }
            );

        uboot-mangopi-debug = ubootMangoPiDebug;
        uboot-mangopi = ubootMangoPi;
      };
    };
}
