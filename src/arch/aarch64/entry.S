/*
 * Copyright (c) 2017, Stefan Lankes, RWTH Aachen University, Germany
 * All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions are met:
 *    * Redistributions of source code must retain the above copyright
 *      notice, this list of conditions and the following disclaimer.
 *    * Redistributions in binary form must reproduce the above copyright
 *      notice, this list of conditions and the following disclaimer in the
 *      documentation and/or other materials provided with the distribution.
 *    * Neither the name of the University nor the names of its contributors
 *      may be used to endorse or promote products derived from this
 *      software without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED
 * WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
 * DISCLAIMED. IN NO EVENT SHALL THE REGENTS OR CONTRIBUTORS BE LIABLE FOR ANY
 * DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES
 * (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES;
 * LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND
 * ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
 * (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS
 * SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 */

/*
 * This is the kernel's entry point, which is derived from Xen's Mini-OS.
 */

#include <hermit/config.h>

#define MAIR(attr, mt)  ((attr) << ((mt) * 8))

#define PAGE_SHIFT	12
#define PAGE_SIZE	(1 << PAGE_SHIFT)
#define PAGE_MASK	(~(PAGE_SIZE-1))

/*
 * Memory types available.
 */
#define MT_DEVICE_nGnRnE    0
#define MT_DEVICE_nGnRE     1
#define MT_DEVICE_GRE       2
#define MT_NORMAL_NC        3
#define MT_NORMAL           4

/*
 * TCR flags
 */
#define TCR_TxSZ(x)     ((((64) - (x)) << 16) | (((64) - (x)) << 0))
#define TCR_IRGN_WBWA   (((1) << 8) | ((1) << 24))
#define TCR_ORGN_WBWA   (((1) << 10) | ((1) << 26))
#define TCR_SHARED      (((3) << 12) | ((3) << 28))
#define TCR_TBI0        ((1) << 37)
#define TCR_TBI1        ((1) << 38)
#define TCR_ASID16      ((1) << 36)
#define TCR_TG1_16K     ((1) << 30)
#define TCR_TG1_4K      ((0) << 30)
#define TCR_FLAGS       (TCR_IRGN_WBWA | TCR_ORGN_WBWA | TCR_SHARED)

/* Number of virtual address bits for 4KB page */
#define VA_BITS         48

#define PT_DEVICE	0x707
#define PT_PT           0x713
#define PT_MEM          0x713
#define PT_MEM_CD	0x70F
#define PT_SELF         ((1) << 55)

#define ALIGN   .align 4

#define END(name) \
	.size name, .-name

#define ENDPROC(name) \
	.type name, @function; \
	END(name)

#define ENTRY(name) \
	.globl name; \
	ALIGN; \
	name:

.section .text

.global _start
_start:
b start64

.align 8
.global base
base: .quad 0
.global limit
limit: .quad 0
.global cpu_freq
cpu_freq: .dword 0
.global boot_processor
boot_processor: .dword 0
.global cpu_online
cpu_online: .dword 0
.global possible_cpus
possible_cpus: .dword 0
.global current_boot_id
current_boot_id: .dword 0
.global isle
isle: .dword -1
.global possible_isles
possible_isles: .dword 1
.global uhyve
uhyve: .dword 0
.global single_kernel
single_kernel: .dword 1
.global image_size
image_size: .quad 0

.global hcip
hcip: .byte 10,0,5,2
.global hcgateway
hcgateway: .byte 10,0,5,1
.global hcmask
hcmask: .byte 255,255,255,0
.global host_logical_addr
host_logical_addr: .quad 0

.global uart_mmio
uart_mmio: .dword 0x9000000		// Initialize with QEMU's UART address

.global cmdline
cmdline: .quad 0
.global cmdsize
cmdsize: .quad 0
.global current_stack_address
current_stack_address: .quad boot_stack_bottom
.global current_percore_address
current_percore_address: .quad PERCORE
.global boot_gtod
boot_gtod: .quad 0

start64:
  //mrs x0, s3_1_c15_c3_0 // Read EL1 Configuration Base Address Register

  /* disable interrupts */
  msr daifset, #0b111

  /* store x5=dtb */
  /*adrp    x1, dtb
  str     x5, [x1, #:lo12:dtb]*/

  /* reset thread id registers */
  mov x0, #0
  msr tpidr_el0, x0
  msr tpidr_el1, x0

  /*
   * Disable the MMU. We may have entered the kernel with it on and
   * will need to update the tables later. If this has been set up
   * with anything other than a VA == PA map then this will fail,
   * but in this case the code to find where we are running from
   * would have also failed.
   */
  dsb sy
  mrs x2, sctlr_el1
  bic x2, x2, #0x1
  msr sctlr_el1, x2
  isb

  /* Calculate where we are */
  //bl _calc_offset

  /* Setup CPU for turnning the MMU on. */
  bl _setup_cpu

  /* Setup the initial page table. */
  bl _setup_pgtable

  /* Load TTBRx */
  mov     x0, xzr
  msr     ttbr1_el1, x0
  ldr     x0, =l0_pgtable
  msr     ttbr0_el1, x0
  isb

  /* Set exception table */
  ldr x0, =vector_table
  msr vbar_el1, x0

  /* Enable hardware coherency between cores */
  /*mrs x0, S3_1_c15_c2_1	// read EL1 CPU Extended Control Register
  orr x0, x0, #(1 << 6)	// set the SMPEN bit
  msr S3_1_c15_c2_1, x0	// write EL1 CPU Extended Control Register
  isb*/

  /* Turning on MMU */
  dsb sy

  /*
   * Prepare system control register (SCTRL)
   *
   *
   *   UCI     [26] Enables EL0 access in AArch64 for DC CVAU, DC CIVAC,
                    DC CVAC and IC IVAU instructions
   *   EE      [25] Explicit data accesses at EL1 and Stage 1 translation
                    table walks at EL1 & EL0 are little-endian
   *   EOE     [24] Explicit data accesses at EL0 are little-endian
   *   WXN     [19] Regions with write permission are not forced to XN
   *   nTWE    [18] WFE instructions are executed as normal
   *   nTWI    [16] WFI instructions are executed as normal
   *   UCT     [15] Enables EL0 access in AArch64 to the CTR_EL0 register
   *   DZE     [14] Execution of the DC ZVA instruction is allowed at EL0
   *   I       [12] Instruction caches enabled at EL0 and EL1
   *   UMA     [9]  Disable access to the interrupt masks from EL0
   *   SED     [8]  The SETEND instruction is available
   *   ITD     [7]  The IT instruction functionality is available
   *   THEE    [6]  ThumbEE is disabled
   *   CP15BEN [5]  CP15 barrier operations disabled
   *   SA0     [4]  Stack Alignment check for EL0 enabled
   *   SA      [3]  Stack Alignment check enabled
   *   C       [2]  Data and unified enabled
   *   A       [1]  Alignment fault checking disabled
   *   M       [0]  MMU enable
   */
  ldr x0, =0x4D5D91D
  msr sctlr_el1, x0

  ldr     x0, =mmu_on
  br      x0

mmu_on:
  /* Pointer to stack base  */
  ldr x1, =(boot_stack+KERNEL_STACK_SIZE-0x10)
  mov sp, x1

  /* Test core ID */
  mrs x0, mpidr_el1

  ldr x0, =cpu_online
  ldr x0, [x0]
  cmp x0, 0
  b.ne application_processor_main

  bl boot_processor_main
  bl halt

smp:
  //bl smp_main

  /* halt */
halt:
  wfe
  b halt

_setup_cpu:
    ic      iallu
    tlbi    vmalle1is
    dsb     ish

    /*
     * Setup memory attribute type tables
     *
     * Memory regioin attributes for LPAE:
     *
     *   n = AttrIndx[2:0]
     *                      n       MAIR
     *   DEVICE_nGnRnE      000     00000000 (0x00)
     *   DEVICE_nGnRE       001     00000100 (0x04)
     *   DEVICE_GRE         010     00001100 (0x0c)
     *   NORMAL_NC          011     01000100 (0x44)
     *   NORMAL             100     11111111 (0xff)
     */
    ldr     x0, =(MAIR(0x00, MT_DEVICE_nGnRnE) | \
                  MAIR(0x04, MT_DEVICE_nGnRE) | \
                  MAIR(0x0c, MT_DEVICE_GRE) | \
                  MAIR(0x44, MT_NORMAL_NC) | \
                  MAIR(0xff, MT_NORMAL))
    msr     mair_el1, x0

    /*
     * Setup translation control register (TCR)
     */

    // determine physical address size
    mrs     x0, id_aa64mmfr0_el1
    and     x0, x0, 0xF
    lsl     x0, x0, 32

    ldr     x1, =(TCR_TxSZ(VA_BITS) | TCR_TG1_4K | TCR_FLAGS )
    orr     x0, x0, x1

    mrs     x1, id_aa64mmfr0_el1
    bfi     x0, x1, #32, #3
    msr     tcr_el1, x0

    /*
     * Enable FP/ASIMD in Architectural Feature Access Control Register,
     */
    mov     x0, #3 << 20
    msr     cpacr_el1, x0

    /*
     * Reset debug controll register
     */
    msr     mdscr_el1, xzr

    ret

_setup_pgtable:
    ldr     x0, =cpu_online
    ldr     x0, [x0]
    cmp     x0, 0
    b.ne    4f

    ldr     x0, =kernel_start
    ldr     x1, =image_size
    ldr     x1, [x1]
    add     x0, x0, x1
    /* align to a 16KByte boundary */
    add     x0, x0, 0x10000
    mov     x1, ~0xFFFF
    and     x0, x0, x1
    mov     x3, x0  // x3 := address of the first allocated page table

    // create page table entries for the 1st GB
    ldr     x1, =l2_pgtable
    add     x1, x1, 8
    add     x0, x0, PT_PT
    mov     x2, xzr
1:
    str     x0, [x1], 8
    add     x0, x0, PAGE_SIZE
    add     x2, x2, 1
    cmp     x2, 511
    b.lo    1b

    // create identity mapping
    ldr     x5, =kernel_start
    mov     x6, x5
    // Create contiguous bit
    mov     x7, 1
    lsl     x7, x7, 52
    add     x7, x7, PT_MEM
    orr     x5, x5, x7
    mov     x0, x3  // map until the first page table
    mov     x7, xzr
2:
    str     x5, [x3], 8
    add     x5, x5, PAGE_SIZE
    add     x6, x6, PAGE_SIZE
    add     x7, x7, 1
    cmp     x6, x0
    b.lo    2b

    /* Clear rest of the boot page tables */
3:
    stp     xzr, xzr, [x3], 16
    stp     xzr, xzr, [x3], 16
    stp     xzr, xzr, [x3], 16
    stp     xzr, xzr, [x3], 16
    add     x7, x7, 8
    cmp     x7, 511*PAGE_SIZE
    b.lo    3b

4:
    ret

//_calc_offset:
//    ldr     x22, =_start             // x0 := vaddr(_start)
//    adr     x21, _start              // x21 := paddr(_start)
//    sub     x22, x22, x21            // x22 := phys-offset (vaddr - paddr)
//    ret

/*
 * There are no PUSH/POP instruction in ARMv8.
 * Use STR and LDR for stack accesses.
 */
.macro push, xreg
str     \xreg, [sp, #-8]!
.endm

.macro pop, xreg
ldr     \xreg, [sp], #8
.endm

.macro trap_entry, el
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

     mrs x22, elr_el1
     mrs x23, spsr_el1
     stp x22, x23, [sp, #-16]!
.endm

.macro trap_exit, el
     ldp x22, x23, [sp], #16
     msr elr_el1, x22
     msr spsr_el1, x23

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
 * SYNC & IRQ exception handler.
 */
.align 6
el1_sync:
      trap_entry 1
      mov     x0, sp
      bl      do_sync
      trap_exit 1
      eret
ENDPROC(el1_sync)

.align 6
el1_irq:
      trap_entry 1
      mov     x0, sp
      bl      do_irq
      cmp     x0, 0
      b.eq    1f

      mov x1, sp
      str x1, [x0]                  /* store old sp */
      bl get_current_stack          /* get new sp   */
      mov sp, x0

      /* call cleanup code */
      bl finish_task_switch

1:    trap_exit 1
      eret
ENDPROC(el1_irq)

.align 6
el1_fiq:
      trap_entry 1
      mov     x0, sp
      bl      do_fiq
      cmp     x0, 0
      b.eq    1f

      mov x1, sp
      str x1, [x0]                  /* store old sp */
      bl get_current_stack          /* get new sp   */
      mov sp, x0

      /* call cleanup code */
      bl finish_task_switch

1:    trap_exit 1
      eret
ENDPROC(el1_fiq)

.align 6
el1_error:
      trap_entry 1
      mov     x0, sp
      bl      do_error
      trap_exit 1
      eret
ENDPROC(el1_error)

/*
 * Bad Abort numbers
 */
#define BAD_SYNC    0
#define BAD_IRQ     1
#define BAD_FIQ     2
#define BAD_ERROR   3

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

el0_sync_invalid:
   invalid BAD_SYNC
ENDPROC(el0_sync_invalid)

el0_irq_invalid:
   invalid BAD_IRQ
ENDPROC(el0_irq_invalid)

el0_fiq_invalid:
   invalid BAD_FIQ
ENDPROC(el0_fiq_invalid)

el0_error_invalid:
   invalid BAD_ERROR
ENDPROC(el0_error_invalid)

el1_sync_invalid:
   invalid BAD_SYNC
ENDPROC(el1_sync_invalid)

el1_irq_invalid:
   invalid BAD_IRQ
ENDPROC(el1_irq_invalid)

el1_fiq_invalid:
   invalid BAD_FIQ
ENDPROC(el1_fiq_invalid)

el1_error_invalid:
   invalid BAD_ERROR
ENDPROC(el1_error_invalid)

.align  11
ENTRY(vector_table)
/* Current EL with SP0 */
ventry el1_sync_invalid	        // Synchronous EL1t
ventry el1_irq_invalid	        // IRQ EL1t
ventry el1_fiq_invalid	        // FIQ EL1t
ventry el1_error_invalid        // Error EL1t

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
END(vector_table)

.section .data
.global boot_stack
.balign 0x10
boot_stack: .skip KERNEL_STACK_SIZE
boot_stack_bottom:

.global l0_pgtable
.align 12
l0_pgtable:
    .quad l1_pgtable + PT_PT
    .space 510*8, 0
    .quad l0_pgtable + PT_PT + PT_SELF
l1_pgtable:
    .quad l2_pgtable + PT_PT
    .space 511*8, 0
l2_pgtable:
    .quad l3_pgtable + PT_PT
    .space 511*8, 0
l3_pgtable:
    .quad 0x00000000 + PT_MEM_CD // map II ports
    .quad 0x09000000 + PT_MEM_CD // map QEMU's uart port
    .space 510*8, 0
