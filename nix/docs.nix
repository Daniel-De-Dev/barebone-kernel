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
            "--document-private-items"
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

      previewDocs = pkgs.writeShellApplication {
        name = "preview-docs";

        runtimeInputs = [
          pkgs.python3
          pkgs.xdg-utils
        ];

        text = ''
          xdg-open "http://127.0.0.1:8000/kernel/index.html" >/dev/null 2>&1 &

          exec python3 -m http.server \
            8000 \
            --bind 127.0.0.1 \
            --directory "$PWD/result"
        '';
      };

      docsCoverage = pkgs.writeShellApplication {
        name = "docs-coverage";

        runtimeInputs = [ rustToolchain ];

        text = ''
          reportDir="target/${rustTarget}/doc"

          rm -f "$reportDir"/*.txt

          RUSTDOCFLAGS="-Z unstable-options --show-coverage -Awarnings" \
            cargo doc \
              --quiet \
              --workspace \
              --no-deps \
              --document-private-items \
              --target "${rustTarget}"

          printf '\nDocumentation coverage\n\n'

          for report in "$reportDir"/*.txt; do
            if [[ ! -f "$report" ]]; then
              continue
            fi

            name="$(basename "$report" .txt)"

            printf '%s\n' "=== $name ==="
            cat "$report"
            printf '\n'
          done
        '';
      };
    in
    {
      packages.docs = docs;
      checks.docs = docs;

      apps = {
        docs-preview = {
          meta.description = "Serves generated documentation locally";
          type = "app";
          program = "${previewDocs}/bin/preview-docs";
        };

        docs-coverage = {
          meta.description = "Generates documentation coverage statistics";
          type = "app";
          program = "${docsCoverage}/bin/docs-coverage";
        };
      };
    };
}
