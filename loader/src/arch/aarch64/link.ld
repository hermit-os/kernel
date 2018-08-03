OUTPUT_FORMAT("elf64-littleaarch64")
OUTPUT_ARCH("aarch64")
ENTRY(start)
phys = 0x40002000;

SECTIONS
{
  kernel_start = phys;
  .text phys : AT(ADDR(.text)) {
    *(.text)
  }
  .rodata ALIGN(4096) : AT(ADDR(.rodata)) {
    *(.rodata)
    *(.rodata.*)
  }
  .data ALIGN(4096) : AT(ADDR(.data)) {
    *(.data)
  }
  .bss ALIGN(4096) : AT(ADDR(.bss)) {
    bss_start = .;
    *(.bss)
  }
  bss_end = .;
  kernel_end = .;
}
