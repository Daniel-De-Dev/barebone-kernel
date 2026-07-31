{ inputs, ... }: {
  perSystem =
    {
      lib,
      pkgs,
      config,
      ...
    }:
    let
      riscvPkgs = pkgs.pkgsCross.riscv64;
      crossCompile = riscvPkgs.stdenv.cc.targetPrefix;

      boards = import ./boards.nix { inherit lib; };

      /*
        Build an OpenSBI FW_DYNAMIC firmware image fir RISC-V.

        `name` identifies the target and forms part of the package name.

        `textStart` sets OpenSBI's FW_TEXT_START.

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

            # FIT image containing OpenSBI and U-Boot
            if [ -f u-boot.itb ]; then
              install -Dm0644 u-boot.itb "$out/u-boot.itb"
            fi

            # QEMU's first-stage U-Boot SPL.
            if [ -f spl/u-boot-spl ]; then
              install -Dm0644 spl/u-boot-spl "$out/u-boot-spl"
            fi

            # VisionFive 2's first-stage U-Boot SPL.
            if [ -f spl/u-boot-spl.bin.normal.out ]; then
              install -Dm0644 \
                spl/u-boot-spl.bin.normal.out \
                "$out/u-boot-spl.bin.normal.out"
            fi

            runHook postInstall
          '';
        };

      /*
        Build U-Boot proper for the MangoPi MQ Pro FEL boot path.

        xfel is capable of initializes DRAM before loading the firmware, so
        this package does not build or install U-Boot SPL.

        Unlike `mkUBoot`, this build does not consume OpenSBI or construct a FIT
        image. The FEL runner loads OpenSBI FW_JUMP, U-Boot proper, and U-Boot's
        device tree into DRAM as separate binaries.

        OpenSBI enters U-Boot proper in supervisor mode and passes it the address
        of the separately loaded device tree.

        `CONFIG_BLKMAP` allows the runner's in-memory EFI system partition to be
        exposed to U-Boot as a block device.

        The package contains only `u-boot.bin` and `u-boot.dtb`.

        # TODO: Document the dtsi step
        # TODO: See what of the documentation belongs in mangopi.nix

        Sources:
        https://xfel.xboot.org/en/command/ddr
        https://docs.u-boot.org/en/stable/usage/blkmap.html
      */
      mkUBootMangoPi =
        { name, espImage }:
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

            espSizeBytes=$(<"${espImage}/disk-size-bytes")
            ramDiskEnd=$((
              ${toString boards.mangopi.ramDiskAddress} + espSizeBytes
            ))
            dramEnd=$((
              ${toString boards.mangopi.dramStart}
              + ${toString boards.mangopi.dramSize}
            ))

            if ((espSizeBytes <= 0)); then
              echo "error: ESP disk image size must be greater than zero" >&2
              exit 1
            fi

            if ((ramDiskEnd > dramEnd)); then
              echo "error: ESP reservation extends beyond MangoPi DRAM" >&2
              exit 1
            fi

            printf -v ramDiskSize '0x%08x' "$espSizeBytes"

            install -Dm0644 \
              ${./dts/mangopi-fel-memory.dtsi.in} \
              arch/riscv/dts/barebone-fel-memory.dtsi

            substituteInPlace arch/riscv/dts/barebone-fel-memory.dtsi \
              --replace-fail '@dramUnitAddress@' '${lib.toHexString boards.mangopi.dramStart}' \
              --replace-fail '@dramStart@' '${boards.toHex boards.mangopi.dramStart}' \
              --replace-fail '@dramSize@' '${boards.toHex boards.mangopi.dramSize}' \
              --replace-fail '@ramDiskUnitAddress@' '${lib.toHexString boards.mangopi.ramDiskAddress}' \
              --replace-fail '@ramDiskAddress@' '${boards.toHex boards.mangopi.ramDiskAddress}' \
              --replace-fail '@ramDiskSize@' "$ramDiskSize"
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
        Build OpenSBI FW_JUMP for the MangoPi MQ Pro FEL boot path.

        TODO: move this explination and the u-boot compilation into mangopi.nix

        `xfel exec` does not supply the FDT address expected in a1, so
        `FW_FDT_PATH` embeds U-Boot's DTB. OpenSBI uses this DTB for generic
        platform discovery, relocates it to `FW_JUMP_FDT_ADDR`, and passes
        that address to U-Boot proper.

        `FW_TEXT_START` sets OpenSBI's link address, chosen to match its FEL
        load address. `FW_JUMP_ADDR` selects U-Boot proper as the next-stage
        entry point.

        Sources:
        https://xfel.xboot.org/en/command/exec
        https://github.com/riscv-software-src/opensbi/blob/c0f87f10d1bfb9e72a84ddfafb5604ee1bfe9d04/docs/firmware/fw_jump.md
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

      ubootMangoPiDebug = mkUBootMangoPi {
        name = "debug";
        espImage = config.packages.esp-image-debug;
      };

      ubootMangoPi = mkUBootMangoPi {
        name = "release";
        espImage = config.packages.esp-image;
      };

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

        uboot-vf2 = mkUBoot {
          name = "visionfive2";
          defconfig = "starfive_visionfive2_defconfig";
          opensbi = opensbiVisionFive2;
        };

        uboot-mangopi-debug = ubootMangoPiDebug;
        uboot-mangopi = ubootMangoPi;
      };
    };
}
