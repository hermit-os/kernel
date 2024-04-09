SECTIONS {
  loader_start = ADDR (.text.start);
  
  .text.start 0x80200000 : { *(.text._start) }
  .text : { *(.text.*) }
  .rodata : { *(.rodata.*) }
  .data : { *(.data.*) }
  .bss : { *(.bss.*) }

  loader_end = .;
}
