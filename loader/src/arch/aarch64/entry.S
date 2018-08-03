/*
 * Copyright (c) 2018 Colin Finck, RWTH Aachen University
 *
 * MIT License
 *
 * Permission is hereby granted, free of charge, to any person obtaining
 * a copy of this software and associated documentation files (the
 * "Software"), to deal in the Software without restriction, including
 * without limitation the rights to use, copy, modify, merge, publish,
 * distribute, sublicense, and/or sell copies of the Software, and to
 * permit persons to whom the Software is furnished to do so, subject to
 * the following conditions:
 *
 * The above copyright notice and this permission notice shall be
 * included in all copies or substantial portions of the Software.
 *
 * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
 * EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
 * MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
 * NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE
 * LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
 * OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION
 * WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
 */

#define KERNEL_STACK_SIZE 4096

#define MAIR(attr, mt)  ((attr) << ((mt) * 8))

/* Memory types */
#define MT_DEVICE_nGnRnE    0
#define MT_DEVICE_nGnRE     1
#define MT_DEVICE_GRE       2
#define MT_NORMAL_NC        3
#define MT_NORMAL           4

/* TCR flags */
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

/* Page Table Entry Flags */
#define PTE_PRESENT             (1 << 0)
#define PTE_TABLE_OR_4KIB_PAGE  (1 << 1)
#define PTE_NORMAL_NC           (0 << 4 | 1 << 3 | 1 << 2)
#define PTE_NORMAL              (1 << 4 | 0 << 3 | 0 << 2)
#define PTE_INNER_SHAREABLE     (1 << 8 | 1 << 9)
#define PTE_ACCESSED            (1 << 10)

/* Combined Page Table Entry flags for different types of entries */
#define PT_PT                   (PTE_PRESENT | PTE_TABLE_OR_4KIB_PAGE | PTE_NORMAL | PTE_INNER_SHAREABLE | PTE_ACCESSED)
#define PT_MEM                  (PTE_PRESENT | PTE_TABLE_OR_4KIB_PAGE | PTE_NORMAL | PTE_INNER_SHAREABLE | PTE_ACCESSED)
#define PT_MEM_2MIB             (PTE_PRESENT | PTE_NORMAL | PTE_INNER_SHAREABLE | PTE_ACCESSED)
#define PT_MEM_CD               (PTE_PRESENT | PTE_TABLE_OR_4KIB_PAGE | PTE_NORMAL_NC | PTE_INNER_SHAREABLE | PTE_ACCESSED)

/* System Control Register (SCTLR_EL1) values */
#define SCTLR_M         (1 << 0)    /* MMU enable */
#define SCTLR_C         (1 << 2)    /* Cache enable */
#define SCTLR_I         (1 << 12)   /* Instruction cache enable */
#define SCTLR_DZE       (1 << 14)   /* Enables access to the DC ZVA instruction at EL0 */
#define SCTLR_UCT       (1 << 15)   /* Enables EL0 access to the CTR_EL0 register in AArch64 Execution state */
#define SCTLR_UCI       (1 << 26)   /* Enables EL0 access to the DC CVAU, DC CIVAC, DC CVAC and IC IVAU instructions in the AArch64 Execution state */


.section .text
.global start
start:
	/* Disable interrupts */
	msr daifset, #0b111

	/* Flush caches */
	ic iallu
	tlbi vmalle1is
	dsb ish

	/* Save the location of the DTB */
	ldr x30, =dtb_address
	str x0, [x30]

	/*
	 * Setup memory attribute type tables
	 *
	 * Memory region attributes for LPAE:
	 *
	 *   n = AttrIndx[2:0]
	 *                      n       MAIR
	 *   DEVICE_nGnRnE      000     00000000 (0x00)
	 *   DEVICE_nGnRE       001     00000100 (0x04)
	 *   DEVICE_GRE         010     00001100 (0x0c)
	 *   NORMAL_NC          011     01000100 (0x44)
	 *   NORMAL             100     11111111 (0xff)
	 */
    ldr x0, =(MAIR(0x00, MT_DEVICE_nGnRnE) | \
		MAIR(0x04, MT_DEVICE_nGnRE) | \
		MAIR(0x0c, MT_DEVICE_GRE) | \
		MAIR(0x44, MT_NORMAL_NC) | \
		MAIR(0xff, MT_NORMAL))
	msr mair_el1, x0

	/* Set up the TCR_EL1 register for paging.
	 * This includes copying the PARange bits from ID_AA64MMFR0_EL1 to the IPS field of TCR_EL1. */
	ldr x0, =(TCR_TxSZ(VA_BITS) | TCR_TG1_4K | TCR_FLAGS)
	mrs x1, id_aa64mmfr0_el1
	bfi x0, x1, #32, #3
	msr tcr_el1, x0

	/* Load TTBRx */
	msr ttbr1_el1, xzr
	ldr x0, =pgtable
	msr ttbr0_el1, x0
	isb

	/* Turn on the MMU and configure the System Control Register (SCTLR_EL1). */
	dsb sy
	mrs x0, sctlr_el1
	ldr x1, =(SCTLR_M | SCTLR_C | SCTLR_I | SCTLR_DZE | SCTLR_UCT | SCTLR_UCI)
	orr x0, x0, x1
	msr sctlr_el1, x0

	/* Set up the stack */
	ldr x30, =(boot_stack+KERNEL_STACK_SIZE-0x10)
	mov sp, x30

	/* Jump to the Rust entry point */
	b loader_main


.section .data
.global dtb_address
dtb_address: .quad 0

.balign 0x10
boot_stack: .skip KERNEL_STACK_SIZE

.align 12
pgtable:
	.quad pgtable_0 + PT_PT         /* Memory from 0x0000_0000_0000_0000 to 0x0000_007F_FFFF_FFFF */
	.space 510*8, 0
	.quad pgtable + PT_PT           /* Self-reference from 0x0000_FF80_0000_0000 to 0x0000_FFFF_FFFF_FFFF */
pgtable_0:
	.quad pgtable_0_0 + PT_PT       /* Memory from 0x0000_0000_0000_0000 to 0x0000_0000_3FFF_FFFF */
	.quad pgtable_0_1 + PT_PT       /* Memory from 0x0000_0000_4000_0000 to 0x0000_0000_7FFF_FFFF */
	.space 510*8, 0
pgtable_0_0:
	.space 72*8, 0
	.quad pgtable_0_0_72 + PT_PT    /* Memory from 0x0000_0900_0000_0000 to 0x0000_09FFF_FFFF_FFFF */
	.space 439*8, 0
pgtable_0_1:
	.quad 0x40000000 + PT_MEM_2MIB  /* Identity-mapping of loader code from 0x0000_0000_4000_0000 to 0x0000_0000_4020_1FFF */
	.space 511*8, 0
pgtable_0_0_72:
	.quad 0x9000000 + PT_MEM_CD     /* Identity-mapping of QEMU PL011 serial port from 0x0000_0900_0000_0000 to 0x0000_0900_0000_0FFF */
	.space 511*8, 0
