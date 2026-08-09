{ inputs, ... }: {
  perSystem =
    {
      pkgs,
      system,
      lib,
      boards,
      ...
    }:
    let
      rustToolchain = import ./toolchain.nix {
        fenixLib = inputs.fenix.packages.${system};
      };

      naersk' = pkgs.callPackage inputs.naersk {
        cargo = rustToolchain;
        rustc = rustToolchain;
      };

      mkKernel =
        {
          name,
          board,
          release,
        }:
        import ./kernel.nix {
          inherit
            lib
            pkgs
            naersk'
            release
            name
            ;

          loadAddress = board.kernelAddress;
          regionSize = board.kernelRegionSize;
        };

      mkKernelPackages = name: board: {
        "kernel-${name}-debug" = mkKernel {
          inherit name board;
          release = false;
        };

        "kernel-${name}" = mkKernel {
          inherit name board;
          release = true;
        };
      };
    in
    {
      packages =
        mkKernelPackages "qemu" boards.qemu
        // mkKernelPackages "vf2" boards.visionfive2
        // mkKernelPackages "mangopi" boards.mangopi;
    };
}
