{ lib }:
let
  MiB = 1024 * 1024;
  GiB = 1024 * MiB;

  toHex = value: "0x${lib.toHexString value}";
in
{
  qemu =
    let
      dramBase = 2 * GiB;
      fitLoadOffset = 2 * MiB;
    in
    {
      /*
        QEMU's RISC-V `virt` machine maps DRAM at 0x80000000.

        Sources:
        https://github.com/qemu/qemu/blob/e1705a25aff35635c360bbaba4c2731d019a422a/hw/riscv/virt.c#L106
      */
      opensbiAddress = toHex dramBase; # 0x80000000

      /*
        U-Boot's qemu-riscv64_spl_defconfig loads the FIT containing
        OpenSBI and U-Boot proper at 0x80200000.

        Sources:
        https://github.com/u-boot/u-boot/blob/100e12ea78c73071b9710f08b32fd4590019266f/configs/qemu-riscv64_spl_defconfig#L14
      */
      fitLoadAddress = toHex (dramBase + fitLoadOffset); # 0x80200000
    };

  visionfive2 =
    let
      base = 1 * GiB;
    in
    {
      /*
        Vision Five 2's DRAM starts at 0x40000000

        Sources:
        https://doc-en.rvspace.org/JH7110/TRM/JH7110_TRM/system_memory_map.html
      */
      opensbiAddress = toHex base; # 0x40000000
    };

  mangopi =
    let
      base = 1 * GiB;
    in
    {
      /*
        MangoPi's MQ-Pro DRAM starts at 0x40000000

        Source:
        d1-h user manual v1.0 - 3.1 Memory Mapping
      */
      opensbiAddress = toHex base; # 0x40000000

      /*
        U-boot's loading address must match `CONFIG_TEXT_BASE` from its u-boot
        config

        Sources:
        https://github.com/smaeul/u-boot/blob/9d8202dd5cab57fa56880179b4e53c79f9ef24a3/board/sunxi/Kconfig#L140-L144
        https://github.com/u-boot/u-boot/blob/ece349ade2973e220f524ce59e59711cc919263f/Kconfig#L690-L708
      */
      ubootAddress = toHex (base + 46 * MiB); # 0x42e00000

      # Other abitrary addresses things will be loaded
      fdtAddress = toHex (base + 64 * MiB); # 0x44000000
      efiLoadAddress = toHex (base + 96 * MiB); # 0x46000000
      ramDiskAddress = toHex (base + 128 * MiB); # 0x48000000

      # TODO: This should match the region reserved in the MangoPi DTB.
      # TODO: And also adjust dynamically based on actualy disk size?
      ramDiskReservedBytes = 64 * MiB;
    };
}
