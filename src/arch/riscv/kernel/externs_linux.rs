// memcpy and memset derived from Linux kernel 5.15.5 /arch/riscv/lib
// memmove and memcmp from Redox (kernel/src/externs.rs) https://gitlab.redox-os.org/redox-os/kernel/-/blob/master/src/externs.rs

use core::arch::asm;
use core::mem;

const WORD_SIZE: usize = mem::size_of::<usize>();

/// Memcpy
///
/// Copy N bytes of memory from one location to another.
#[no_mangle]
#[naked]
pub unsafe extern "C" fn memcpy(dest: *mut u8, src: *const u8, n: usize) {
	asm!(
		"move t6, a0",
		"sltiu a3, a2, 128",
		"bnez a3, 4f",
		"andi a3, t6, 8-1",
		"andi a4, a1, 8-1",
		"bne a3, a4, 4f",
		"beqz a3, 2f",
		"andi a3, a1, ~(8-1)",
		"addi a3, a3, 8",
		"sub a4, a3, a1",
		"1: lb a5, 0(a1)",
		"addi a1, a1, 1",
		"sb a5, 0(t6)",
		"addi t6, t6, 1",
		"bltu a1, a3, 1b",
		"sub a2, a2, a4",
		"2: andi a4, a2, ~((16*8)-1)",
		"beqz a4, 4f",
		"add a3, a1, a4",
		"3: ld a4,       0(a1)",
		"ld a5,   8(a1)",
		"ld a6, 2*8(a1)",
		"ld a7, 3*8(a1)",
		"ld t0, 4*8(a1)",
		"ld t1, 5*8(a1)",
		"ld t2, 6*8(a1)",
		"ld t3, 7*8(a1)",
		"ld t4, 8*8(a1)",
		"ld t5, 9*8(a1)",
		"sd a4,       0(t6)",
		"sd a5,   8(t6)",
		"sd a6, 2*8(t6)",
		"sd a7, 3*8(t6)",
		"sd t0, 4*8(t6)",
		"sd t1, 5*8(t6)",
		"sd t2, 6*8(t6)",
		"sd t3, 7*8(t6)",
		"sd t4, 8*8(t6)",
		"sd t5, 9*8(t6)",
		"ld a4, 10*8(a1)",
		"ld a5, 11*8(a1)",
		"ld a6, 12*8(a1)",
		"ld a7, 13*8(a1)",
		"ld t0, 14*8(a1)",
		"ld t1, 15*8(a1)",
		"addi a1, a1, 16*8",
		"sd a4, 10*8(t6)",
		"sd a5, 11*8(t6)",
		"sd a6, 12*8(t6)",
		"sd a7, 13*8(t6)",
		"sd t0, 14*8(t6)",
		"sd t1, 15*8(t6)",
		"addi t6, t6, 16*8",
		"bltu a1, a3, 3b",
		"andi a2, a2, (16*8)-1",
		"4: beqz a2, 6f",
		"add a3, a1, a2",
		"or a5, a1, t6",
		"or a5, a5, a3",
		"andi a5, a5, 3",
		"bnez a5, 5f",
		"7: lw a4, 0(a1)",
		"addi a1, a1, 4",
		"sw a4, 0(t6)",
		"addi t6, t6, 4",
		"bltu a1, a3, 7b",
		"ret",
		"5: lb a4, 0(a1)",
		"addi a1, a1, 1",
		"sb a4, 0(t6)",
		"addi t6, t6, 1",
		"bltu a1, a3, 5b",
		"6: ret",
		options(noreturn),
	);
}

/// Memmove
///
/// Copy N bytes of memory from src to dest. The memory areas may overlap.
#[no_mangle]
pub unsafe extern "C" fn memmove(dest: *mut u8, src: *const u8, n: usize) -> *mut u8 {
	if src < dest as *const u8 {
		let n_usize: usize = n / WORD_SIZE; // Number of word sized groups
		let mut i: usize = n_usize * WORD_SIZE;

		// Copy `WORD_SIZE` bytes at a time
		while i != 0 {
			i -= WORD_SIZE;
			*((dest as usize + i) as *mut usize) = *((src as usize + i) as *const usize);
		}

		let mut i: usize = n;

		// Copy 1 byte at a time
		while i != n_usize * WORD_SIZE {
			i -= 1;
			*((dest as usize + i) as *mut u8) = *((src as usize + i) as *const u8);
		}
	} else {
		let n_usize: usize = n / WORD_SIZE; // Number of word sized groups
		let mut i: usize = 0;

		// Copy `WORD_SIZE` bytes at a time
		let n_fast = n_usize * WORD_SIZE;
		while i < n_fast {
			*((dest as usize + i) as *mut usize) = *((src as usize + i) as *const usize);
			i += WORD_SIZE;
		}

		// Copy 1 byte at a time
		while i < n {
			*((dest as usize + i) as *mut u8) = *((src as usize + i) as *const u8);
			i += 1;
		}
	}

	dest
}

/// Memset
///
/// Fill a block of memory with a specified value.
#[no_mangle]
#[naked]
pub unsafe extern "C" fn memset(dest: *mut u8, c: i32, n: usize) -> *mut u8 {
	asm!(
		"move t0, a0",
		"sltiu a3, a2, 16",
		"bnez a3, 4f",
		"addi a3, t0, 8-1",
		"andi a3, a3, ~(8-1)",
		"beq a3, t0, 2f",
		"sub a4, a3, t0",
		"1: sb a1, 0(t0)",
		"addi t0, t0, 1",
		"bltu t0, a3, 1b",
		"sub a2, a2, a4",
		"2: andi a1, a1, 0xff",
		"slli a3, a1, 8",
		"or a1, a3, a1",
		"slli a3, a1, 16",
		"or a1, a3, a1",
		"slli a3, a1, 32",
		"or a1, a3, a1",
		"andi a4, a2, ~(8-1)",
		"add a3, t0, a4",
		"andi a4, a4, 31*8",
		"beqz a4, 3f",
		"neg a4, a4",
		"addi a4, a4, 32*8",
		"sub t0, t0, a4",
		"la a5, 3f",
		"srli a4, a4, 1",
		"add a5, a5, a4",
		"jr a5",
		"3: sd a1,        0(t0)",
		"sd a1,    8(t0)",
		"sd a1,  2*8(t0)",
		"sd a1,  3*8(t0)",
		"sd a1,  4*8(t0)",
		"sd a1,  5*8(t0)",
		"sd a1,  6*8(t0)",
		"sd a1,  7*8(t0)",
		"sd a1,  8*8(t0)",
		"sd a1,  9*8(t0)",
		"sd a1, 10*8(t0)",
		"sd a1, 11*8(t0)",
		"sd a1, 12*8(t0)",
		"sd a1, 13*8(t0)",
		"sd a1, 14*8(t0)",
		"sd a1, 15*8(t0)",
		"sd a1, 16*8(t0)",
		"sd a1, 17*8(t0)",
		"sd a1, 18*8(t0)",
		"sd a1, 19*8(t0)",
		"sd a1, 20*8(t0)",
		"sd a1, 21*8(t0)",
		"sd a1, 22*8(t0)",
		"sd a1, 23*8(t0)",
		"sd a1, 24*8(t0)",
		"sd a1, 25*8(t0)",
		"sd a1, 26*8(t0)",
		"sd a1, 27*8(t0)",
		"sd a1, 28*8(t0)",
		"sd a1, 29*8(t0)",
		"sd a1, 30*8(t0)",
		"sd a1, 31*8(t0)",
		"addi t0, t0, 32*8",
		"bltu t0, a3, 3b",
		"andi a2, a2, 8-1",
		"4: beqz a2, 6f",
		"add a3, t0, a2",
		"5: sb a1, 0(t0)",
		"addi t0, t0, 1",
		"bltu t0, a3, 5b",
		"6: ret",
		options(noreturn),
	);
}

/// Memcmp
///
/// Compare two blocks of memory.
///
/// This faster implementation works by comparing bytes not one-by-one, but in
/// groups of 8 bytes (or 4 bytes in the case of 32-bit architectures).
#[no_mangle]
pub unsafe extern "C" fn memcmp(s1: *const u8, s2: *const u8, n: usize) -> i32 {
	let n_usize: usize = n / WORD_SIZE;
	let mut i: usize = 0;

	let n_fast = n_usize * WORD_SIZE;
	while i < n_fast {
		let a = *((s1 as usize + i) as *const usize);
		let b = *((s2 as usize + i) as *const usize);
		if a != b {
			let n: usize = i + WORD_SIZE;
			// Find the one byte that is not equal
			while i < n {
				let a = *((s1 as usize + i) as *const u8);
				let b = *((s2 as usize + i) as *const u8);
				if a != b {
					return a as i32 - b as i32;
				}
				i += 1;
			}
		}
		i += WORD_SIZE;
	}

	while i < n {
		let a = *((s1 as usize + i) as *const u8);
		let b = *((s2 as usize + i) as *const u8);
		if a != b {
			return a as i32 - b as i32;
		}
		i += 1;
	}

	0
}
