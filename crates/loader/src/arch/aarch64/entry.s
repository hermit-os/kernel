// Adapted from https://github.com/rust-embedded/rust-raspberrypi-OS-tutorials/blob/master/02_runtime_init/src/_arch/aarch64/cpu/boot.s

.equ _core_id_mask, 0xff

.section .text._start

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
	
	// Jump to Rust code.
	b	_start_rust

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