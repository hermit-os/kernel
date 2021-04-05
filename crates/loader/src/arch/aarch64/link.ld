OUTPUT_FORMAT("elf64-littleaarch64")
OUTPUT_ARCH("aarch64")
ENTRY(_start)

/* start address of the RAM, below belongs to the flash */
phys = 0x00080000;

SECTIONS
{
  . = phys;
  kernel_start = .;
  .text ALIGN(4096) : AT(ADDR(.text)) {
    *(.text)
    *(.text.*)
  }
  .rodata ALIGN(4096) : AT(ADDR(.rodata)) {
    *(.rodata)
    *(.rodata.*)
  }
  .data ALIGN(4096) : AT(ADDR(.data)) {
    *(.data)
    *(.data.*)
  }
  .bss ALIGN(4096) : AT(ADDR(.bss)) {
    bss_start = .;
    *(.bss)
    *(.bss.*)
  }
  bss_end = .;
  kernel_end = .;
}
