/* Flat arm64 Linux image, loaded by Firecracker at DRAM start + 2 MiB */
ENTRY(_start)
phys = 0x80200000;

SECTIONS
{
  __executable_start = phys;
  .text phys : {
    KEEP(*(.text.head))
    *(.text)
    *(.text.*)
  }
  .rodata ALIGN(4096) : {
    *(.rodata)
    *(.rodata.*)
  }
  .data ALIGN(4096) : {
    *(.data)
    *(.data.*)
  }
  .bss ALIGN(4096) : {
    *(.bss)
    *(.bss.*)
  }
  _end = .;
  /DISCARD/ : {
    *(.eh_frame)
    *(.eh_frame_hdr)
  }
}
