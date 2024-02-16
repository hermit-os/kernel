use core::arch::asm;

use crate::syscalls::*;

/// number of the system call `exit`
const SYSNO_EXIT: usize = 0;
/// number of the system call `write`
const SYSNO_WRITE: usize = 1;
/// number of the system call `read`
const SYSNO_READ: usize = 2;
/// number of the system call `abort`
const SYSNO_ABORT: usize = 3;
/// number of the system call `usleep`
const SYSNO_USLEEP: usize = 4;
/// number of the system call `getpid`
const SYSNO_GETPID: usize = 5;
/// number of the system call `yield`
const SYSNO_YIELD: usize = 6;
/// number of the system call `read_entropy`
const SYSNO_READ_ENTROPY: usize = 7;
/// number of the system call `get_processor_count`
const SYSNO_GET_PROCESSOR_COUNT: usize = 8;
/// number of the system call `close`
const SYSNO_CLOSE: usize = 9;
/// number of the system call `futex_wait`
const SYSNO_FUTEX_WAIT: usize = 10;
/// number of the system call `futex_wake`
const SYSNO_FUTEX_WAKE: usize = 11;
/// number of the system call `open`
const SYSNO_OPEN: usize = 12;

/// total number of system calls
const NO_SYSCALLS: usize = 32;

extern "C" fn invalid_syscall(sys_no: u64) -> ! {
	error!("Invalid syscall {}", sys_no);
	sys_exit(1);
}

#[allow(unused_assignments)]
#[no_mangle]
#[naked]
pub(crate) unsafe extern "C" fn sys_invalid() {
	unsafe {
		asm!(
			"mov rdi, rax",
			"call {}",
			sym invalid_syscall,
			options(noreturn)
		);
	}
}

#[repr(align(64))]
#[repr(C)]
pub(crate) struct SyscallTable {
	handle: [*const usize; NO_SYSCALLS],
}

impl SyscallTable {
	pub const fn new() -> Self {
		let mut table = SyscallTable {
			handle: [sys_invalid as *const _; NO_SYSCALLS],
		};

		table.handle[SYSNO_EXIT] = sys_exit as *const _;
		table.handle[SYSNO_WRITE] = sys_write as *const _;
		table.handle[SYSNO_READ] = sys_read as *const _;
		table.handle[SYSNO_ABORT] = sys_abort as *const _;
		table.handle[SYSNO_USLEEP] = sys_usleep as *const _;
		table.handle[SYSNO_GETPID] = sys_getpid as *const _;
		table.handle[SYSNO_YIELD] = sys_yield as *const _;
		table.handle[SYSNO_READ_ENTROPY] = sys_read_entropy as *const _;
		table.handle[SYSNO_GET_PROCESSOR_COUNT] = sys_get_processor_count as *const _;
		table.handle[SYSNO_CLOSE] = sys_close as *const _;
		table.handle[SYSNO_FUTEX_WAIT] = sys_futex_wait as *const _;
		table.handle[SYSNO_FUTEX_WAKE] = sys_futex_wake as *const _;
		table.handle[SYSNO_OPEN] = sys_open as *const _;

		table
	}
}

unsafe impl Send for SyscallTable {}
unsafe impl Sync for SyscallTable {}

#[no_mangle]
pub(crate) static SYSHANDLER_TABLE: SyscallTable = SyscallTable::new();
