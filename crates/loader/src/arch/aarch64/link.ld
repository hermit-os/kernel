/* Parts of this linker script are directly taken from Andre Richters Project:
 * https://github.com/rust-embedded/rust-raspberrypi-OS-tutorials/blob/master/16_virtual_mem_part4_higher_half_kernel/src/bsp/raspberrypi/link.ld 
*/

OUTPUT_FORMAT("elf64-littleaarch64")
OUTPUT_ARCH("aarch64")
ENTRY(_start)
phys = 0x40200000;

PHDRS
{
  segment_ro PT_LOAD FLAGS(4); /* 4 == RO */
  segment_rx PT_LOAD FLAGS(5); /* 5 == RX */
  segment_rw PT_LOAD FLAGS(6); /* 6 == RW */
}

SECTIONS
{
  . = phys;
  kernel_start = .;
  .text : {
    KEEP(*(.text._start))
    *(.text)
    *(.text.*)
  } :segment_rx
  .rodata : ALIGN(8) {
    *(.rodata)
    *(.rodata.*)
  } :segment_ro
  .got    : ALIGN(8) {
    /* Global offset table Todo */
    *(.got)
  } :segment_ro
  .data   : ALIGN(8) {
    *(.data)
    *(.data.*)
  } :segment_rw
  .bss    : ALIGN(8) {
    *(.bss)
    *(.bss.*)
  } :segment_rw
  . = ALIGN(4K); /* Align to page boundary */
  /***********************************************************************************************
   * Boot Core Stack
   ***********************************************************************************************/
  __boot_core_stack_start = .;         /*   ^             */ 
                                       /*   | stack       */
  . += 16K;                            /*   | growth      */
                                       /*   | direction   */
  __boot_core_stack_end_exclusive = .; /*   |             */
  kernel_end = .;
}
