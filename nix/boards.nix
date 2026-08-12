{ lib, ... }: {
  perSystem =
    _:
    let
      MiB = 1024 * 1024;
      GiB = 1024 * MiB;

      toHex = value: "0x${lib.toHexString value}";

      # TODO: implement more robust region checks/verification assertions

      # Project-defined memory-layout offsets relative to DRAM base
      kernelOffset = 1 * MiB; # 0x00100000
      kernelRegionSize = 1 * MiB;

      # TODO: Move fdt closer to kernel and add asserts for overlap check
      fdtOffset = 64 * MiB; # 0x04000000
    in
    {
      _module.args.boards = {
        inherit toHex;

        qemu =
          let
            /*
              QEMU's RISC-V `virt` machine maps DRAM at 0x80000000.

              Sources:
              https://github.com/qemu/qemu/blob/e1705a25aff35635c360bbaba4c2731d019a422a/hw/riscv/virt.c#L106
            */
            dramBase = 2 * GiB; # 0x80000000
          in
          {
            inherit kernelRegionSize;

            opensbiAddress = dramBase;
            kernelAddress = dramBase + kernelOffset;
          };

        visionfive2 =
          let
            /*
              VisionFive 2's DDR address space starts at 0x40000000.

              Sources:
              https://doc-en.rvspace.org/JH7110/TRM/JH7110_TRM/system_memory_map.html
            */
            dramBase = 1 * GiB; # 0x40000000
          in
          {
            inherit kernelRegionSize;

            opensbiAddress = dramBase;
            kernelAddress = dramBase + kernelOffset;
            fdtAddress = dramBase + fdtOffset;

            # The BootROM UART recovery protocol uses 115200 baud
            baudrateBootROM = 115200;

            /*
              750000 baud is the highest rate currently usable with my CP2102
              USB-to-UART adapter. The VisionFive 2 UART itself supports higher
              rates.

              TODO: Document the procedure used to configure the CP2102 for
              750000 baud.

              Read more:
              https://forum.rvspace.org/t/increase-serial-port-baudrate/4557/2
            */
            baudrate = 750000;
          };

        mangopi =
          let
            /*
              The D1-H DRAM address space starts at 0x40000000.

              This project currently targets the 512 MiB MangoPi MQ-Pro variant.

              Source:
              d1-h user manual v1.0 - 3.1 Memory Mapping
            */
            dramBase = 1 * GiB; # 0x40000000

            # RAM capacity of the targeted MangoPi MQ-Pro variant
            dramSize = 512 * MiB; # 0x20000000
          in
          {
            inherit kernelRegionSize;

            inherit dramBase dramSize;
            opensbiAddress = dramBase;

            kernelAddress = dramBase + kernelOffset;
            fdtAddress = dramBase + fdtOffset;
            baudrate = 115200;
          };
      };
    };
}
