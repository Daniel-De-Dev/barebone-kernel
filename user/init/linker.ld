ENTRY(_start)

SECTIONS
{
  . = 0x10000 - 32;
  .image_header : {
    QUAD(0x3130524553554242); /* BBUSER01 */
    QUAD(_image_start);
    QUAD(_start);
    QUAD(_image_end - _image_start);
  }

  _image_start = .;
  .text : {
    KEEP(*(.text.start))
    *(.text .text.*)
  }
  .rodata : {
    *(.srodata .srodata.*)
    *(.rodata .rodata.*)
  }
  _image_end = .;

  __global_pointer$ = _image_start + 0x800;

  .data : { *(.sdata .sdata.*) *(.data .data.*) *(.got .got.*) }
  .bss (NOLOAD) : { *(.sbss .sbss.*) *(.bss .bss.*) *(COMMON) }

  /DISCARD/ : { *(.eh_frame .eh_frame.*) *(.eh_frame_hdr) *(.comment) }
}

ASSERT((_image_start % 4096) == 0, "user image must start on a page boundary")
ASSERT(_start == _image_start, "entry must begin the user payload")
ASSERT(_image_end > _image_start, "user image must contain executable code")
ASSERT(SIZEOF(.data) == 0, "read-only user images cannot contain writable data")
ASSERT(SIZEOF(.bss) == 0, "read-only user images cannot contain BSS")
