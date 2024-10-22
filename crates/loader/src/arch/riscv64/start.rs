use core::ptr;
use core::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};

use fdt::Fdt;

static mut STACK: Stack = Stack::new();
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

pub fn get_stack_ptr() -> *mut u8 {
	// SAFETY: We only create a pointer here
	let stack_top = ptr::addr_of_mut!(STACK);
	// SAFETY: Pointing directly past the object is allowed
	let stack_bottom = unsafe { stack_top.add(1) };
	stack_bottom.cast::<u8>()
}

// TODO: Migrate to Constrained Naked Functions once stabilized
// https://github.com/rust-lang/rust/issues/90957
// TODO: Migrate to asm_const for Stack::SIZE once stabilized
// https://github.com/rust-lang/rust/issues/93332
#[no_mangle]
#[naked_function::naked]
pub unsafe extern "C" fn _start(hart_id: usize, fdt: *const u8) -> ! {
	asm!(
		// Initialize stack
		"la      sp, {BOOT_STACK}",
		"li      t0, 0x8000",
		"add     sp, sp, t0",

		"j       {start}",

		BOOT_STACK = sym STACK,
		start = sym start,
	)
}

extern "C" fn start(hart_id: usize, fdt: *const u8) -> ! {
	HART_ID.store(hart_id, Ordering::Relaxed);
	FDT.store(fdt.cast_mut(), Ordering::Relaxed);

	unsafe { crate::os::loader_main() }
}

// Align to page size
#[repr(C, align(0x1000))]
pub struct Stack([u8; Self::SIZE]);

impl Stack {
	const SIZE: usize = 0x8000;

	pub const fn new() -> Self {
		Self([0; Self::SIZE])
	}
}
