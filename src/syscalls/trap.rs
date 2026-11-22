//! Delivery of synchronous exceptions (traps) to a handler of the application.
//!
//! The kernel does not call the handler within the exception. Instead, it
//! returns from the exception to `trap_entry`, which calls the handler in the
//! context of the interrupted task. The handler may leave `trap_entry` without
//! returning, e.g. to resume the execution at another location. If it returns,
//! the task is aborted.

use core::mem;
use core::sync::atomic::{AtomicUsize, Ordering};

use memory_addresses::VirtAddr;

#[cfg(target_arch = "riscv64")]
use crate::arch::mm::paging::try_virtual_to_physical as virtual_to_physical;
#[cfg(not(target_arch = "riscv64"))]
use crate::arch::mm::paging::virtual_to_physical;
use crate::arch::mm::paging::{BasePageSize, PageSize};
use crate::scheduler;

pub(crate) const TRAP_ILLEGAL_INSTRUCTION: u32 = 1;
pub(crate) const TRAP_MEMORY_FAULT: u32 = 2;
#[cfg_attr(not(target_arch = "x86_64"), expect(dead_code))]
pub(crate) const TRAP_ARITHMETIC: u32 = 3;

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct TrapInfo {
	pub kind: u32,
	pub has_faulting_addr: bool,
	pub pc: usize,
	pub fp: usize,
	pub sp: usize,
	pub faulting_addr: usize,
}

pub type TrapHandler = unsafe extern "C" fn(info: &TrapInfo);

static TRAP_HANDLER: AtomicUsize = AtomicUsize::new(0);

/// Registers a process-wide handler for synchronous exceptions.
/// `None` restores the default behavior.
#[hermit_macro::system(errno)]
#[unsafe(no_mangle)]
pub extern "C" fn sys_set_trap_handler(handler: Option<TrapHandler>) -> i32 {
	TRAP_HANDLER.store(
		handler.map_or(0, |handler| handler as usize),
		Ordering::Release,
	);
	0
}

/// Checks whether a trap can be delivered with the stack pointer `sp`.
///
/// This requires a registered handler and mapped memory from one page below
/// `sp`, on which `trap_entry` and the handler run, up to `top`.
pub(crate) fn can_deliver(sp: usize, top: usize) -> bool {
	if TRAP_HANDLER.load(Ordering::Acquire) == 0 {
		return false;
	}

	let page_size = BasePageSize::SIZE as usize;
	let Some(low) = sp.checked_sub(page_size) else {
		return false;
	};
	let mapped = |addr: usize| virtual_to_physical(VirtAddr::new(addr as u64)).is_some();
	(low..top).step_by(page_size).all(mapped) && mapped(top - 1)
}

/// Address, at which the interrupted task continues after the exception.
///
/// The task has to pass the fields of the `TrapInfo` as arguments.
#[cfg(not(target_arch = "x86_64"))]
pub(crate) fn entry() -> usize {
	(trap_entry as *const ()).expose_provenance()
}

extern "C" fn trap_entry(
	kind: u32,
	has_faulting_addr: bool,
	pc: usize,
	fp: usize,
	sp: usize,
	faulting_addr: usize,
) -> ! {
	let info = TrapInfo {
		kind,
		has_faulting_addr,
		pc,
		fp,
		sp,
		faulting_addr,
	};

	let handler = TRAP_HANDLER.load(Ordering::Acquire);
	if handler != 0 {
		// SAFETY: only valid handlers are stored in `TRAP_HANDLER`.
		let handler = unsafe { mem::transmute::<usize, TrapHandler>(handler) };
		// SAFETY: the application registered the handler for this purpose.
		unsafe { handler(&info) };
	}

	error!("Unhandled trap: {info:#x?}");
	scheduler::abort()
}

#[cfg(target_arch = "x86_64")]
pub(crate) use self::x86_64::*;

#[cfg(target_arch = "x86_64")]
mod x86_64 {
	/// Number of values, which `trap_trampoline` pops from the stack.
	pub(crate) const TRAMPOLINE_ARGS: usize = 6;

	/// Loads the arguments of `trap_entry` from the stack.
	///
	/// The stack contains `kind`, `has_faulting_addr`, `pc`, an unused value,
	/// `sp` and `faulting_addr`. `rbp` still contains the frame pointer of the
	/// interrupted code.
	#[unsafe(naked)]
	pub(crate) unsafe extern "C" fn trap_trampoline() -> ! {
		core::arch::naked_asm!(
			"pop rdi",
			"pop rsi",
			"pop rdx",
			"pop rcx",
			"mov rcx, rbp",
			"pop r8",
			"pop r9",
			"call {entry}",
			"ud2",
			entry = sym super::trap_entry,
		)
	}
}
