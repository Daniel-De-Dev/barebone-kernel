{ inputs, ... }: {
  perSystem =
    {
      config,
      pkgs,
      system,
      lib,
      boards,
      ...
    }:
    let
      variants = import ./logging.nix;

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
          loggingFeatures,
        }:
        import ./kernel.nix {
          inherit
            lib
            pkgs
            naersk'
            release
            name
            loggingFeatures
            ;

          userImage = config.packages.user-init;

          loadAddress = board.kernelAddress;
          inherit (board) kernelOffset;
          regionSize = board.kernelRegionSize;
        };

      mkKernelPackages =
        name: board:
        lib.concatMapAttrs (suffix: variant: {
          "kernel-${name}${suffix}" = mkKernel {
            inherit name board;
            inherit (variant) release loggingFeatures;
          };
        }) variants;
    in
    {
      packages = {
        user-init = import ./user.nix { inherit pkgs naersk'; };
      }
      // mkKernelPackages "qemu" boards.qemu
      // mkKernelPackages "vf2" boards.visionfive2
      // mkKernelPackages "mangopi" boards.mangopi;
    };
}
