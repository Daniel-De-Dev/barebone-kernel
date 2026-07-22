{ naersk', ... }:
naersk'.buildPackage {
  src = ./../..;
  cargoBuildOptions =
    defaultOpts:
    defaultOpts
    ++ [
      "-p"
      "kernel"
    ];
  CARGO_BUILD_TARGET = "riscv64gc-unknown-none-elf";
}
