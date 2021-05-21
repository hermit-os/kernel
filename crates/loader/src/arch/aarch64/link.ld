/* Parts of this linker script are directly taken from Andre Richters Project:
 * https://github.com/rust-embedded/rust-raspberrypi-OS-tutorials/blob/master/16_virtual_mem_part4_higher_half_kernel/src/bsp/raspberrypi/link.ld 
*/

OUTPUT_FORMAT("elf64-littleaarch64")
OUTPUT_ARCH("aarch64")
ENTRY(_start)

/* start address of the RAM, below belongs to the flash */
phys = 0x00080000;

PHDRS
{
    segment_rx PT_LOAD FLAGS(5); /* 5 == RX */
    segment_rw PT_LOAD FLAGS(6); /* 6 == RW */
}

SECTIONS
{
  . = phys;
  kernel_start = .;
  __rx_start = .;
  .text ALIGN(4096) : AT(phys) {
      KEEP(*(.text._start))
      *(.text._start_arguments) /* Constants (or statics in Rust speak) read by _start(). */
      *(.text._start_rust)      /* The Rust entry point */
      *(.text*)                 /* Everything else */
  } :segment_rx
  .rodata : ALIGN(8) { *(.rodata*) } :segment_rx
  .got    : ALIGN(8) { *(.got)     } :segment_rx /* Global offset table Todo - do we use this?*/
  . = ALIGN(64K); /* Align to page boundary */
  __rx_end_exclusive = .;

  __rw_start = .;
  .data : { *(.data*) } :segment_rw

  .bss : ALIGN(8) {
    bss_start = .;
    *(.bss*);
    . = ALIGN(8);
    . += 8;
    bss_end = .;
  } :NONE

  . = ALIGN(64K); /* Align to page boundary */
  __rw_end_exclusive = .;

  /***********************************************************************************************
  * Guard Page between boot core stack and data
  ***********************************************************************************************/
  __boot_core_stack_guard_page_start = .;
  . += 64K;
  __boot_core_stack_guard_page_end_exclusive = .;

  /***********************************************************************************************
  * Boot Core Stack
  ***********************************************************************************************/
  __boot_core_stack_start = .;         /*   ^             */
                                       /*   | stack       */
  . += 512K;                           /*   | growth      */
                                       /*   | direction   */
  __boot_core_stack_end_exclusive = .; /*   |             */
  kernel_end = .;
}
