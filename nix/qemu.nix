{ inputs, ... }: {
  perSystem =
    {
      config,
      pkgs,
      lib,
      boards,
      ...
    }:
    let
      opensbiLib = import ./opensbi.nix { inherit inputs lib pkgs; };

      /*
        QEMU loads OpenSBI as machine firmware and places the kernel at its
        fixed load address in DRAM. OpenSBI FW_JUMP then enters the kernel
        at that address in S-mode.

        QEMU's `virt` machine provides its generated FDT to OpenSBI.

        Sources:
        https://www.qemu.org/docs/master/system/riscv/virt.html#hardware-configuration-information
      */
      opensbiQemu = opensbiLib.mkJump {
        name = "jump-qemu";
        textStart = boards.qemu.opensbiAddress;
        jumpAddress = boards.qemu.kernelAddress;
      };

      /*
        Run the kernel on QEMU's RISC-V `virt` machine.

        OpenSBI FW_JUMP is installed as the machine firmware with `-bios`.
        QEMU's generic loader places the raw kernel image at the address
        for which it was linked. The loader does not change the CPU entry
        point; execution begins in OpenSBI, which later jumps to the
        kernel.

        Sources:
        https://www.qemu.org/docs/master/system/riscv/virt.html
        https://www.qemu.org/docs/master/system/generic-loader.html
      */
      mkRunQemu =
        {
          programName,
          kernel,
          opensbi,
        }:
        pkgs.writeShellApplication {
          name = programName;

          runtimeInputs = [
            pkgs.coreutils
            pkgs.qemu
          ];

          runtimeEnv = {
            OPENSBI_IMAGE = "${opensbi}/fw_jump.bin";
            KERNEL_IMAGE = "${kernel}/bin/kernel.bin";
            KERNEL_ADDRESS = boards.toHex kernel.loadAddress;
          };

          text = ''
            # shellcheck source=/dev/null
            source ${./scripts/common.sh}
            ${lib.removePrefix "set -euo pipefail\n" (
              builtins.readFile ./scripts/run-qemu.sh
            )}
          '';
        };
    in
    {
      packages = {
        # TODO: Rename all "nix run .#run-*" to no longer start with run
        run-qemu-debug = mkRunQemu {
          programName = "run-qemu-debug";
          kernel = config.packages.kernel-qemu-debug;
          opensbi = opensbiQemu;
        };

        run-qemu = mkRunQemu {
          programName = "run-qemu";
          kernel = config.packages.kernel-qemu;
          opensbi = opensbiQemu;
        };
      };

      checks.qemu-boot-smoke =
        pkgs.runCommand "qemu-boot-smoke"
          {
            nativeBuildInputs = [
              pkgs.coreutils
              pkgs.gnugrep
            ];
          }
          ''
            log="$TMPDIR/qemu.log"

            set +e
            timeout 5s ${config.packages.run-qemu}/bin/run-qemu \
              >"$log" 2>&1
            status=$?
            set -e

            if [ "$status" -ne 124 ]; then
              echo "QEMU exited unexpectedly with status $status" >&2
              cat "$log" >&2
              exit 1
            fi

            if ! grep -Fq "kernel entered" "$log"; then
              echo "kernel did not reach main" >&2
              cat "$log" >&2
              exit 1
            fi

            touch "$out"
          '';
    };
}
