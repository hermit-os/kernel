// Adapted from https://github.com/rust-embedded/rust-raspberrypi-OS-tutorials/blob/master/02_runtime_init/src/_arch/aarch64/cpu/boot.s

.equ _core_id_mask, 0xff

.section .text

_start:
	// Only proceed on the boot core. Park it otherwise.
	mrs	x1, mpidr_el1
	and	x1, x1, _core_id_mask
	mov	x2, xzr  // Assume CPU 0 is responsible for booting
	cmp	x1, x2
	b.ne	1f

	// If execution reaches here, it is the boot core. Now, prepare the jump to Rust code.

	// This loads the physical address of the stack end. For details see
	// https://github.com/rust-embedded/rust-raspberrypi-OS-tutorials/blob/master/16_virtual_mem_part4_higher_half_kernel/src/bsp/raspberrypi/link.ld
	adrp	x4, __boot_core_stack_end_exclusive
	add		x4, x4, #:lo12:__boot_core_stack_end_exclusive
	mov		sp, x4

	// Set correct Exception level!

	mrs	x0, CurrentEL
	cmp	x0, #4  // EL = 1
	b.eq	el_1_entry

	// Test if EL2
	cmp	x0, #8
	b.eq	el_2_entry

	//EL3
	msr	SP_EL2, x4
	msr	SP_EL1, x4
	msr	SCTLR_EL2, xzr
	msr	HCR_EL2, xzr

	mrs	x0, SCR_EL3
	and	x0, x0, #(~(1 << 3))
	and	x0, x0, #(~(1 << 2))
	and	x0, x0, #(~(1 << 1))
	orr	x0, x0, #(1<<10)
	orr	x0, x0, #(1<<0)
	msr	SCR_EL3, x0

	mov	x0, #0b1111001001 // D-Flag, I-FLAG, A-FLAG, F-FLAG, EL2h
	msr	SPSR_EL3, x0

	adr	x0, el_2_entry
	msr	ELR_EL3, x0

	eret

// EL2
el_2_entry:
	msr	SP_EL1, x4
	msr	SCTLR_EL1, xzr
	mrs	x0, HCR_EL2
	orr	x0, x0, #(1<<31)
	msr	HCR_EL2, x0

	mov	x0, #0b1111000101 // D-Flag, I-FLAG, A-FLAG, F-FLAG, EL1h
	msr	SPSR_EL2, x0

	adr	x0, el_1_entry
	msr	ELR_EL2, x0

	eret

	// Jump to Rust code.
el_1_entry:
	b	{start_rust}

	// Infinitely wait for events (aka "park the core").
1:	wfe
	b	1b

.size	_start, . - _start
.type	_start, function
.global	_start

.section .bss

.global l0_pgtable
.global l1_pgtable
.global l2_pgtable
.global l2k_pgtable
.global l3_pgtable
.global L0mib_pgtable

.align 12
l0_pgtable:
    .space 512*8, 0
l1_pgtable:
    .space 512*8, 0
l2_pgtable:
    .space 512*8, 0
l2k_pgtable:
    .space 512*8, 0
l3_pgtable:
    .space 512*8, 0
L0mib_pgtable:
    .space 512*8, 0
L2mib_pgtable:
    .space 512*8, 0
L4mib_pgtable:
    .space 512*8, 0
L6mib_pgtable:
    .space 512*8, 0
L8mib_pgtable:
    .space 512*8, 0
L10mib_pgtable:
    .space 512*8, 0
L12mib_pgtable:
    .space 512*8, 0
L14mib_pgtable:
    .space 512*8, 0
L16mib_pgtable:
    .space 512*8, 0
L18mib_pgtable:
    .space 512*8, 0
