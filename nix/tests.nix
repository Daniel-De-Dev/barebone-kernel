{ inputs, ... }: {
  perSystem =
    { pkgs, system, ... }:
    let
      rustToolchain = import ./toolchain.nix {
        fenixLib = inputs.fenix.packages.${system};
      };

      naersk' = pkgs.callPackage inputs.naersk {
        cargo = rustToolchain;
        rustc = rustToolchain;
      };

      workspace-clippy = naersk'.buildPackage {
        pname = "workspace-clippy";
        version = "0.1.0";
        src = ./..;

        mode = "clippy";

        cargoBuildOptions =
          options:
          options
          ++ [
            "--workspace"
            "--target"
            "riscv64gc-unknown-none-elf"
          ];

        cargoClippyOptions = _: [ ];
      };

      workspace-tests = naersk'.buildPackage {
        pname = "workspace-tests";
        version = "0.1.0";
        src = ./..;
        mode = "test";
        cargoTestOptions = options: options ++ [ "--workspace" ];
      };

      run-tests = pkgs.writeShellApplication {
        name = "run-tests";
        runtimeInputs = [ rustToolchain ];
        text = ''
          cargo test --workspace
        '';
      };

      run-coverage = pkgs.writeShellApplication {
        name = "run-coverage";
        runtimeInputs = [
          rustToolchain
          pkgs.cargo-llvm-cov
          pkgs.xdg-utils
        ];
        text = ''
          cargo llvm-cov --workspace --html --open
        '';
      };
    in
    {
      checks = { inherit workspace-tests workspace-clippy; };

      apps = {
        test = {
          meta.description = "Runs the tests in workspace";
          type = "app";
          program = "${run-tests}/bin/run-tests";
        };
        test-coverage = {
          meta.description = "Generates and opens a report on test coverage";
          type = "app";
          program = "${run-coverage}/bin/run-coverage";
        };
      };
    };
}
