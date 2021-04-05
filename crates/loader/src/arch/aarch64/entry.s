// Adapted from https://github.com/rust-embedded/rust-raspberrypi-OS-tutorials/blob/master/02_runtime_init/src/_arch/aarch64/cpu/boot.s

.equ _core_id_mask, 0b11    //Assume 4 core raspi3

.section .text._start

_start:
	// Only proceed on the boot core. Park it otherwise.
	mrs	x1, MPIDR_EL1
	and	x1, x1, _core_id_mask
	mov	x2, 0      // Assume CPU 0 is responsible for booting
	cmp	x1, x2
	b.ne	1f

	// If execution reaches here, it is the boot core. Now, prepare the jump to Rust code.

	// Jump to Rust code.
	b	_start_rust

	// Infinitely wait for events (aka "park the core").
1:	wfe
	b	1b

.size	_start, . - _start
.type	_start, function
.global	_start