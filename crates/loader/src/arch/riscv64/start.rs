use core::ptr;
use core::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};

use fdt::Fdt;

use crate::stack::{STACK, Stack};

static HART_ID: AtomicUsize = AtomicUsize::new(0);
static FDT: AtomicPtr<u8> = AtomicPtr::new(ptr::null_mut());

pub fn get_hart_id() -> usize {
	HART_ID.load(Ordering::Relaxed)
}

pub fn get_fdt_ptr() -> *const u8 {
	FDT.load(Ordering::Relaxed).cast_const()
}

pub fn get_fdt() -> Fdt<'static> {
	// SAFETY: We trust the FDT pointer provided by the firmware
	unsafe { Fdt::from_ptr(get_fdt_ptr()).unwrap() }
}

// TODO: Migrate to Constrained Naked Functions once stabilized
// https://github.com/rust-lang/rust/issues/90957
// TODO: Migrate to asm_const for Stack::SIZE once stabilized
// https://github.com/rust-lang/rust/issues/93332
#[no_mangle]
#[naked_function::naked]
#[link_section = ".init"]
pub unsafe extern "C" fn _start(hart_id: usize, fdt: *const u8) -> ! {
	asm!(
		// Initialize stack
		"la      sp, {BOOT_STACK}",
		"li      t0, {STACK_SIZE}",
		"add     sp, sp, t0",

		"j       {start}",

		BOOT_STACK = sym STACK,
		STACK_SIZE = const Stack::SIZE,
		start = sym start,
	)
}

extern "C" fn start(hart_id: usize, fdt: *const u8) -> ! {
	crate::log::init();
	HART_ID.store(hart_id, Ordering::Relaxed);
	FDT.store(fdt.cast_mut(), Ordering::Relaxed);

	unsafe { crate::os::loader_main() }
}
