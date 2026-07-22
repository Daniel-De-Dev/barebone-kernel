{ inputs, ... }: {
  perSystem =
    {
      pkgs,
      system,
      lib,
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
    in
    {
      packages = rec {
        bootloader = import ./bootloader.nix {
          inherit
            lib
            pkgs
            naersk'
            rustToolchain
            ;
        };
        # TODO: Uncomment once ready to work on kernel and build properly defined
        # kernel = import ./kernel.nix { inherit pkgs naersk'; };

        default = bootloader;
      };
    };
}
