{ inputs, ... }: {
  perSystem =
    { lib, pkgs, ... }:
    let
      riscvPkgs = pkgs.pkgsCross.riscv64;
      crossCompile = riscvPkgs.stdenv.cc.targetPrefix;

      /*
        Build OpenSBI's dynamic firmware.

        `name` is used to give dynamic package name (pname)
        `textStart` is the address at which OpenSBI is loaded:
          - QEMU virt: 0x80000000
          - VisionFive 2: 0x40000000

        References:
          https://github.com/riscv-software-src/opensbi/blob/master/docs/firmware/fw_dynamic.md
          https://github.com/riscv-software-src/opensbi/blob/master/docs/platform/qemu_virt.md
          https://doc-en.rvspace.org/VisionFive2/SWTRM/VisionFive2_SW_TRM/compiling_opensbi%20-%20vf2.html
      */
      mkOpenSBI =
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

      opensbiQemu = mkOpenSBI {
        name = "qemu";
        textStart = "0x80000000";
      };

      opensbiVisionFive2 = mkOpenSBI {
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

            # VisionFive 2 uses the FIT image containing OpenSBI and U-Boot.
            if [ -f u-boot.itb ]; then
              install -Dm0644 u-boot.itb "$out/u-boot.itb"
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
    in
    {
      packages = {
        opensbi-qemu = opensbiQemu;
        opensbi-vf2 = opensbiVisionFive2;

        uboot-qemu = mkUBoot {
          name = "qemu";
          defconfig = "qemu-riscv64_smode_defconfig";
          opensbi = opensbiQemu;
        };

        uboot-vf2 = mkUBoot {
          name = "visionfive2";
          defconfig = "starfive_visionfive2_defconfig";
          opensbi = opensbiVisionFive2;
        };
      };
    };
}
