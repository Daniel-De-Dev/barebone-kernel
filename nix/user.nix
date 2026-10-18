{ pkgs, naersk' }:
naersk'.buildPackage {
  pname = "user-init";
  version = "0.1.0";
  src = ./..;
  release = true;

  cargoBuildOptions =
    options:
    options
    ++ [
      "--package"
      "user-init"
    ];
  CARGO_BUILD_TARGET = "riscv64gc-unknown-none-elf";
  nativeBuildInputs = [ pkgs.llvmPackages.bintools ];

  postInstall = ''
    llvm-objcopy -O binary "$out/bin/init" "$out/bin/init.bin"
    mv -- "$out/bin/init" "$out/bin/init.elf"
  '';
}
