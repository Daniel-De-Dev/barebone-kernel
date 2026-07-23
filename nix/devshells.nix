{ inputs, ... }: {
  perSystem =
    { pkgs, system, ... }:
    let
      toolchain = import ./toolchain.nix {
        fenixLib = inputs.fenix.packages.${system};
      };
    in
    {
      devShells.default = pkgs.mkShell {
        packages = [ toolchain ];

        shellHook = ''
          echo "TODO: FIX ECHO WITH USEFUL INFO"
        '';
      };
    };
}
