{
  description = "Barebone RISC-V kernel targeting VisionFive 2 & MangoPi";

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

    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    naersk = {
      url = "github:nix-community/naersk";
      inputs.nixpkgs.follows = "nixpkgs";
      inputs.fenix.follows = "fenix";
    };

    src-uboot = {
      url = "github:u-boot/u-boot";
      flake = false;
    };

    # RISC-V Allwinner D1 support does not exists for mainline U-Boot.
    src-uboot-d1 = {
      url = "github:smaeul/u-boot/d1-wip";
      flake = false;
    };

    src-opensbi = {
      url = "github:riscv-software-src/opensbi";
      flake = false;
    };
  };

  outputs =
    inputs@{ flake-parts, ... }:
    flake-parts.lib.mkFlake { inherit inputs; } {
      systems = [ "x86_64-linux" ];

      imports = [
        inputs.standards.flakeModules.default
        ./nix/firmware.nix
        ./nix/packages.nix
        ./nix/devshells.nix
        ./nix/qemu.nix
        ./nix/visionfive2.nix
        ./nix/mangopi.nix
        ./nix/esp-image.nix
      ];
    };
}
