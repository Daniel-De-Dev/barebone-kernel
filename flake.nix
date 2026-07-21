{
  description = "A bare minimum flake using flake-parts and nixos-standards";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
    flake-parts.url = "github:hercules-ci/flake-parts";

    standards = {
      url = "github:Daniel-De-Dev/flake-standards";
      inputs = {
        flake-parts.follows = "flake-parts";
        nixpkgs.follows = "nixpkgs";
      };
    };
  };

  outputs =
    inputs@{ flake-parts, ... }:
    flake-parts.lib.mkFlake { inherit inputs; } {
      systems = [ "x86_64-linux" ];

      imports = [ inputs.standards.flakeModules.default ];

      perSystem = _: {
        # Local project configurations go here
      };
    };
}
