SECTIONS {
  kernel_start = ADDR (.text.start);
  
  .text.start 0x80200000 : { *(.text._start) }
  .text : { *(.text.*) }
  .rodata : { *(.rodata.*) }
  .data : { *(.data.*) }
  .bss : { *(.bss.*) }

  kernel_end = .;
}
