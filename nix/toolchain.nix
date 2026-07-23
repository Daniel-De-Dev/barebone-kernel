{ fenixLib }:
fenixLib.combine [
  fenixLib.latest.toolchain
  fenixLib.latest.rust-src
  fenixLib.targets."riscv64gc-unknown-none-elf".latest.toolchain
]
