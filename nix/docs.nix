{ inputs, ... }: {
  perSystem =
    { pkgs, system, ... }:
    let
      rustTarget = "riscv64gc-unknown-none-elf";

      rustToolchain = import ./toolchain.nix {
        fenixLib = inputs.fenix.packages.${system};
      };

      naersk' = pkgs.callPackage inputs.naersk {
        cargo = rustToolchain;
        rustc = rustToolchain;
      };

      docs = naersk'.buildPackage {
        pname = "rust-workspace-docs";
        version = "0.1.0";
        src = ./..;

        mode = "check";
        release = false;

        copyBins = false;
        copyLibs = false;

        CARGO_BUILD_TARGET = rustTarget;

        cargoBuildOptions = options: options ++ [ "--workspace" ];

        doDoc = true;
        doDocFail = true;
        copyDocsToSeparateOutput = false;

        cargoDocOptions =
          options:
          options
          ++ [
            "--workspace"
            "--no-deps"
          ];

        preDoc = ''
          mkdir -p target/doc
        '';

        postInstall = ''
          mkdir -p "$out"

          cp -r \
            "target/${rustTarget}/doc/." \
            "$out/"

          echo '<meta http-equiv="refresh" content="0; url=kernel/index.html">' \
            > "$out/index.html"
        '';
      };

      openDocs = pkgs.writeShellApplication {
        name = "open-docs";

        runtimeInputs = [ pkgs.xdg-utils ];

        text = ''
          exec xdg-open "${docs}/index.html"
        '';
      };
    in
    {
      packages.docs = docs;
      checks.docs = docs;

      apps.docs = {
        type = "app";
        program = "${openDocs}/bin/open-docs";
      };
    };
}
