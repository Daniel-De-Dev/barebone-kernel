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

      mkBootloader =
        {
          name,
          board,
          release,
        }:
        import ./bootloader.nix {
          inherit
            lib
            pkgs
            naersk'
            release
            name
            ;

          loadAddress = board.bootloaderAddress;
          regionSize = board.bootloaderRegionSize;
        };

      mkBootloaderPackages = name: board: {
        "bootloader-${name}-debug" = mkBootloader {
          inherit name board;
          release = false;
        };

        "bootloader-${name}" = mkBootloader {
          inherit name board;
          release = true;
        };
      };
    in
    {
      packages =
        mkBootloaderPackages "qemu" boards.qemu
        // mkBootloaderPackages "vf2" boards.visionfive2
        // mkBootloaderPackages "mangopi" boards.mangopi;

      # TODO:...
      # kernel = import ./kernel.nix ...;
    };
}
