{ inputs, ... }: {
  perSystem =
    { lib, pkgs, ... }:
    let
      riscvPkgs = pkgs.pkgsCross.riscv64;
      crossCompile = riscvPkgs.stdenv.cc.targetPrefix;

      # TODO: Centralize pointer/address information so theyre named and documeted better

      /*
        Build an OpenSBI dynamic firmware image.

        `name` is used to give the package its pname.
        `textStart` is the address at which OpenSBI is loaded:
          - QEMU virt: 0x80000000
          - VisionFive 2 / Allwinner D1: 0x40000000

        References:
          https://github.com/riscv-software-src/opensbi/blob/master/docs/firmware/fw.md
          https://github.com/riscv-software-src/opensbi/blob/master/docs/platform/qemu_virt.md
          https://doc-en.rvspace.org/VisionFive2/SWTRM/VisionFive2_SW_TRM/compiling_opensbi%20-%20vf2.html
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

          makeFlags = [
            "ARCH=riscv"
            "CROSS_COMPILE=${crossCompile}"
            "PLATFORM=generic"
            "FW_TEXT_START=${textStart}"
          ];

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
        textStart = "0x80000000";
      };

      opensbiVisionFive2 = mkOpenSBIDynamic {
        name = "visionfive2";
        textStart = "0x40000000";
      };

      /*
        Build U-Boot using an OpenSBI FW_DYNAMIC image.

        References:
          https://docs.u-boot.org/en/stable/board/starfive/visionfive2.html
          https://docs.u-boot.org/en/stable/board/emulation/qemu-riscv.html
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

          # TODO: Switch to using env lik in the other (make sure to test)
          makeFlags = [
            "ARCH=riscv"
            "CROSS_COMPILE=${crossCompile}"
            "DTC=${lib.getExe pkgs.dtc}"
          ];

          configurePhase = ''
            runHook preConfigure

            make -j$NIX_BUILD_CORES ${defconfig}

            runHook postConfigure
          '';

          env.OPENSBI = "${opensbi}/fw_dynamic.bin";

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
        MangoPi U-Boot proper only.

        xfel already initializes DRAM, so this target deliberately does not
        build or package SPL and does not require an fw image.
        OpenSBI receives the separate DTB and jumps to u-boot proper
      */
      ubootMangoPi = riscvPkgs.stdenv.mkDerivation {
        pname = "u-boot-mangopi-fel";
        version = inputs.src-uboot-d1.shortRev or "dirty";
        src = inputs.src-uboot-d1;

        patches = [ ./patches/mangopi-mq-pro-512m-memory.patch ];

        postPatch = ''
          patchShebangs scripts tools
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
        xfel initializes the D1 DDR3 controller and loads the firmware stages
        directly into DRAM.

        xfel can begin execution at the OpenSBI address, but it does not provide
        the DTB address through the RISC-V a1 register. Embed U-Boot's DTB into
        OpenSBI so that OpenSBI can initialize the D1 platform and UART.

        OpenSBI copies the embedded DTB to 0x44000000, passes that address to
        U-Boot in a1, and then enters U-Boot at 0x42e00000.

        References:
          https://xfel.xboot.org/en/reference/ddr-types
          https://github.com/riscv-software-src/opensbi/blob/master/docs/firmware/fw.md
      */
      opensbiMangoPiFel = riscvPkgs.stdenv.mkDerivation {
        pname = "opensbi-jump-mangopi-fel";
        version = inputs.src-opensbi.shortRev or "dirty";
        src = inputs.src-opensbi;

        nativeBuildInputs = [ pkgs.python3 ];

        postPatch = ''
          patchShebangs scripts
        '';

        enableParallelBuilding = true;

        makeFlags = [
          "ARCH=riscv"
          "CROSS_COMPILE=${crossCompile}"
          "PLATFORM=generic"
          "FW_JUMP=y"
          "FW_TEXT_START=0x40000000"
          "FW_JUMP_ADDR=0x42e00000"
          "FW_FDT_PATH=${ubootMangoPi}/u-boot.dtb"
          "FW_JUMP_FDT_ADDR=0x44000000"
        ];

        installPhase = ''
          runHook preInstall

          install -Dm0644 \
            build/platform/generic/firmware/fw_jump.bin \
            "$out/fw_jump.bin"

          runHook postInstall
        '';
      };
    in
    {
      packages = {
        opensbi-qemu = opensbiQemu;
        opensbi-vf2 = opensbiVisionFive2;
        opensbi-mangopi-fel = opensbiMangoPiFel;

        /*
          QEMU is compiled and launched with SPL to better align with the
          hardware boot sequence.

          https://www.qemu.org/docs/master/system/riscv/virt.html#running-u-boot
        */
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

        uboot-mangopi = ubootMangoPi;
      };
    };
}
