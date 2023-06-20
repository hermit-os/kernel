.section .text
.extern do_bad_mode
.extern do_irq
.extern do_fiq
.extern do_sync
.extern do_error
.extern get_last_stack_pointer

.macro trap_entry spsel
     stp x29, x30, [sp, #-16]!
     stp x27, x28, [sp, #-16]!
     stp x25, x26, [sp, #-16]!
     stp x23, x24, [sp, #-16]!
     stp x21, x22, [sp, #-16]!
     stp x19, x20, [sp, #-16]!
     stp x17, x18, [sp, #-16]!
     stp x15, x16, [sp, #-16]!
     stp x13, x14, [sp, #-16]!
     stp x11, x12, [sp, #-16]!
     stp x9, x10, [sp, #-16]!
     stp x7, x8, [sp, #-16]!
     stp x5, x6, [sp, #-16]!
     stp x3, x4, [sp, #-16]!
     stp x1, x2, [sp, #-16]!

     mrs x22, tpidr_el0
     stp x22, x0, [sp, #-16]!

     mrs x23, sp_el0
     mrs x22, spsr_el1
     stp x22, x23, [sp, #-16]!

     mrs x23, elr_el1
     mov x22, #\spsel
     stp x22, x23, [sp, #-16]!
.endm

.macro trap_exit
     ldp x22, x23, [sp], #16
     msr elr_el1, x23

     ldp x22, x23, [sp], #16
     msr spsr_el1, x22
     msr sp_el0, x23

     ldp x22, x0, [sp], #16
     msr tpidr_el0, x22

     ldp x1, x2, [sp], #16
     ldp x3, x4, [sp], #16
     ldp x5, x6, [sp], #16
     ldp x7, x8, [sp], #16
     ldp x9, x10, [sp], #16
     ldp x11, x12, [sp], #16
     ldp x13, x14, [sp], #16
     ldp x15, x16, [sp], #16
     ldp x17, x18, [sp], #16
     ldp x19, x20, [sp], #16
     ldp x21, x22, [sp], #16
     ldp x23, x24, [sp], #16
     ldp x25, x26, [sp], #16
     ldp x27, x28, [sp], #16
     ldp x29, x30, [sp], #16
 .endm

/*
 * Exception vector entry
 */
.macro ventry label
.align  7
b       \label
.endm

.macro invalid, reason
mov     x0, sp
mov     x1, #\reason
b       do_bad_mode
.endm

/*
 * SYNC exception handler.
 */
.align 6
el1_sync:
      trap_entry 1
      mov     x0, sp
      bl      do_sync
      trap_exit
      eret
      // speculation barrier after the ERET to prevent the CPU
      // from speculating past the exception return.
      dsb     nsh
      isb
.size el1_sync, .-el1_sync
.type el1_sync, @function

/*
 * IRQ handler.
 */
.align 6
el1_irq:
      trap_entry 1
      mov     x0, sp
      bl      do_irq
      cmp x0, 0
      b.eq 1f
      // switch to the next task
      mov x1, sp
      str x1, [x0]                  /* store old sp */
      bl get_last_stack_pointer     /* get new sp   */
      mov sp, x0
1:
      trap_exit
      eret
      // speculation barrier after the ERET to prevent the CPU
      // from speculating past the exception return.
      dsb     nsh
      isb
.size el1_irq, .-el1_irq
.type el1_irq, @function

/*
 * FIQ handler.
 */
.align 6
el1_fiq:
      trap_entry 1
      mov     x0, sp
      bl      do_fiq
      cmp x0, 0
      b.eq 2f
      // switch to the next task
      mov x1, sp
      str x1, [x0]                  /* store old sp */
      bl get_last_stack_pointer     /* get new sp   */
      mov sp, x0
2:
      trap_exit
      eret
      // speculation barrier after the ERET to prevent the CPU
      // from speculating past the exception return.
      dsb     nsh
      isb
.size el1_fiq, .-el1_fiq
.type el1_fiq, @function

.align 6
el1_error:
      trap_entry 1
      mov     x0, sp
      bl      do_error
      trap_exit
      eret
      // speculation barrier after the ERET to prevent the CPU
      // from speculating past the exception return.
      dsb     nsh
      isb
.size el1_error, .-el1_error
.type el1_error, @function

/*
 * SYNC exception handler with SP0.
 */
.align 6
el1_sp0_sync:
      msr spsel, #1
      trap_entry 0
      mov     x0, sp
      bl      do_sync
      trap_exit
      eret
      // speculation barrier after the ERET to prevent the CPU
      // from speculating past the exception return.
      dsb     nsh
      isb
.size el1_sp0_sync, .-el1_sp0_sync
.type el1_sp0_sync, @function

/*
 * IRQ handler with SP0.
 */
.align 6
el1_sp0_irq:
      msr spsel, #1
      trap_entry 0
      mov     x0, sp
      bl      do_irq
      cmp x0, 0
      b.eq 3f
      // switch to the next task
      mov x1, sp
      str x1, [x0]                  /* store old sp */
      bl get_last_stack_pointer     /* get new sp   */
      mov sp, x0
3:
      trap_exit
      eret
      // speculation barrier after the ERET to prevent the CPU
      // from speculating past the exception return.
      dsb     nsh
      isb
.size el1_sp0_irq, .-el1_sp0_irq
.type el1_sp0_irq, @function

/*
 * FIQ handler with SP0.
 */
.align 6
el1_sp0_fiq:
      msr spsel, #1
      trap_entry 0
      mov     x0, sp
      bl      do_fiq
      cmp x0, 0
      b.eq 4f
      // switch to the next task
      mov x1, sp
      str x1, [x0]                  /* store old sp */
      bl get_last_stack_pointer     /* get new sp   */
      mov sp, x0
4:
      trap_exit
      eret
      // speculation barrier after the ERET to prevent the CPU
      // from speculating past the exception return.
      dsb     nsh
      isb
.size el1_sp0_fiq, .-el1_sp0_fiq
.type el1_sp0_fiq, @function

.align 6
el1_sp0_error:
      msr spsel, #1
      trap_entry 0
      mov     x0, sp
      bl      do_error
      trap_exit
      eret
      // speculation barrier after the ERET to prevent the CPU
      // from speculating past the exception return.
      dsb     nsh
      isb
.size el1_sp0_error, .-el1_sp0_error
.type el1_sp0_error, @function

el0_sync_invalid:
   invalid 0
.type el0_sync_invalid, @function

el0_irq_invalid:
   invalid 1
.type el0_irq_invalid, @function

el0_fiq_invalid:
   invalid 2
.type el0_fiq_invalid, @function

el0_error_invalid:
   invalid 3
.type el0_error_invalid, @function

el1_sync_invalid:
   invalid 0
.type el1_sync_invalid, @function

el1_irq_invalid:
   invalid 1
.type el1_irq_invalid, @function

el1_fiq_invalid:
   invalid 2
.type el1_fiq_invalid, @function

el1_error_invalid:
   invalid 3
.type el1_error_invalid, @function

/* start of the data section */
.section .rodata
.align  11
.global vector_table
vector_table:
/* Current EL with SP0 */
ventry el1_sp0_sync  	        // Synchronous EL1t
ventry el1_sp0_irq	        // IRQ EL1t
ventry el1_sp0_fiq   	        // FIQ EL1t
ventry el1_sp0_error            // Error EL1t

/* Current EL with SPx */
ventry el1_sync                 // Synchronous EL1h
ventry el1_irq                  // IRQ EL1h
ventry el1_fiq                  // FIQ EL1h
ventry el1_error                // Error EL1h

/* Lower EL using AArch64 */
ventry el0_sync_invalid         // Synchronous 64-bit EL0
ventry el0_irq_invalid          // IRQ 64-bit EL0
ventry el0_fiq_invalid          // FIQ 64-bit EL0
ventry el0_error_invalid        // Error 64-bit EL0

/* Lower EL using AArch32 */
ventry el0_sync_invalid         // Synchronous 32-bit EL0
ventry el0_irq_invalid          // IRQ 32-bit EL0
ventry el0_fiq_invalid          // FIQ 32-bit EL0
ventry el0_error_invalid        // Error 32-bit EL0
.size vector_table, .-vector_table
