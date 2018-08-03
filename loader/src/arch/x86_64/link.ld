OUTPUT_FORMAT("elf64-x86-64")
OUTPUT_ARCH("i386:x86-64")
ENTRY(start)
phys = 0x000000100000;

SECTIONS
{
  kernel_start = phys;
  .mboot phys : AT(ADDR(.mboot)) {
    *(.mboot)
  }
  .text ALIGN(4096) : AT(ADDR(.text)) {
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
