{ lib }:
let
  MiB = 1024 * 1024;
  GiB = 1024 * MiB;

  toHex = value: "0x${lib.toHexString value}";

  # Project standardized offsets
  efiLoadOffset = 96 * MiB; # 0x06000000
  ramDiskOffset = 128 * MiB; # 0x08000000
in
{
  inherit toHex;

  # Memory allocated for the disk in memory when UART/FEL booting
  ramDiskSize = 2 * MiB;

  qemu =
    let
      dramBase = 2 * GiB; # 0x80000000
    in
    {
      /*
        QEMU's RISC-V `virt` machine maps DRAM at 0x80000000.

        Sources:
        https://github.com/qemu/qemu/blob/e1705a25aff35635c360bbaba4c2731d019a422a/hw/riscv/virt.c#L106
      */
      opensbiAddress = dramBase;

      /*
        U-Boot's qemu-riscv64_spl_defconfig loads the FIT containing
        OpenSBI and U-Boot proper at 0x80200000.

        Sources:
        https://github.com/u-boot/u-boot/blob/100e12ea78c73071b9710f08b32fd4590019266f/configs/qemu-riscv64_spl_defconfig#L14
      */
      fitLoadAddress = dramBase + 2 * MiB; # 0x80200000
    };

  visionfive2 =
    let
      base = 1 * GiB; # 0x40000000
    in
    {
      /*
        Vision Five 2's DRAM starts at 0x40000000

        Sources:
        https://doc-en.rvspace.org/JH7110/TRM/JH7110_TRM/system_memory_map.html
      */
      opensbiAddress = base;
      efiLoadAddress = base + efiLoadOffset;
      ramDiskAddress = base + ramDiskOffset;

      # BootROM's baudrate is hardcoded
      baudrateBootROM = 115200;

      /*
        Due to a combination of vf2's UART0 and my usb-to-uart limitations
        this is the highest speed configurable. With a better usb-to-uart
        module, i could theoretically reach a baudrate of 1500000 on UART0

        TODO: Document the procedure to configure CP2102 to support 750000 baud

        Read more:
        https://forum.rvspace.org/t/increase-serial-port-baudrate/4557
      */
      baudrate = 750000;
    };

  mangopi =
    let
      base = 1 * GiB; # 0x40000000
      size = 512 * MiB; # 0x20000000
    in
    {
      /*
        MangoPi's MQ-Pro DRAM starts at 0x40000000

        Source:
        d1-h user manual v1.0 - 3.1 Memory Mapping
      */
      dramStart = base;
      dramSize = size;
      opensbiAddress = base;

      baudrate = 115200;

      /*
        U-boot's loading address must match `CONFIG_TEXT_BASE` from its u-boot
        config

        Sources:
        https://github.com/smaeul/u-boot/blob/9d8202dd5cab57fa56880179b4e53c79f9ef24a3/board/sunxi/Kconfig#L140-L144
        https://github.com/u-boot/u-boot/blob/ece349ade2973e220f524ce59e59711cc919263f/Kconfig#L690-L708
      */
      ubootAddress = base + 46 * MiB; # 0x42e00000

      fdtAddress = base + 64 * MiB; # 0x44000000
      efiLoadAddress = base + efiLoadOffset;
      ramDiskAddress = base + ramDiskOffset;
    };
}
