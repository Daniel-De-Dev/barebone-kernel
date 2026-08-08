{ fenixLib }:
fenixLib.combine [
  fenixLib.latest.toolchain
  fenixLib.targets."riscv64gc-unknown-none-elf".latest.rust-std
]
