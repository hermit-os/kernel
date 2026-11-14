use core::arch::naked_asm;

use crate::mm::vma::sys_mmap;
#[cfg(any(feature = "net", feature = "virtio-vsock"))]
use crate::syscalls::socket::addrinfo::*;
#[cfg(any(feature = "net", feature = "virtio-vsock"))]
use crate::syscalls::socket::*;
use crate::syscalls::*;

/// Number of the system call `exit`
const SYSNO_EXIT: usize = 0;
/// Number of the system call `write`
const SYSNO_WRITE: usize = 1;
/// Number of the system call `read`
const SYSNO_READ: usize = 2;
/// Number of the system call `usleep`
const SYSNO_USLEEP: usize = 3;
/// Number of the system call `getpid`
const SYSNO_GETPID: usize = 4;
/// Number of the system call `yield`
const SYSNO_YIELD: usize = 5;
/// Number of the system call `read_entropy`
const SYSNO_READ_ENTROPY: usize = 6;
/// Number of the system call `get_processor_count`
const SYSNO_GET_PROCESSOR_COUNT: usize = 7;
/// Number of the system call `close`
const SYSNO_CLOSE: usize = 8;
/// Number of the system call `futex_wait`
const SYSNO_FUTEX_WAIT: usize = 9;
/// Number of the system call `futex_wake`
const SYSNO_FUTEX_WAKE: usize = 10;
/// Number of the system call `open`
const SYSNO_OPEN: usize = 11;
/// Number of the system call `writev`
const SYSNO_WRITEV: usize = 12;
/// Number of the system call `readv`
const SYSNO_READV: usize = 13;
const SYSNO_WAITPID: usize = 15;
const SYSNO_SPAWN_PROCESS: usize = 16;
const SYSNO_CLOCK_GETTIME: usize = 17;
const SYSNO_SPAWN: usize = 18;
const SYSNO_SPAWN2: usize = 19;
const SYSNO_JOIN: usize = 20;
const SYSNO_UNLINK: usize = 21;
const SYSNO_MKDIR: usize = 22;
const SYSNO_RMDIR: usize = 23;
const SYSNO_STAT: usize = 24;
const SYSNO_LSTAT: usize = 25;
const SYSNO_FSTAT: usize = 26;
const SYSNO_DUP: usize = 27;
const SYSNO_IOCTL: usize = 28;
const SYSNO_POLL: usize = 29;
const SYSNO_BLOCK_CURRENT_TASK: usize = 35;
const SYSNO_BLOCK_CURRENT_TASK_WITH_TIMEOUT: usize = 36;
const SYSNO_WAKEUP_TASK: usize = 37;
#[cfg(any(feature = "net", feature = "virtio-vsock"))]
const SYSNO_SOCKET: usize = 38;
#[cfg(any(feature = "net", feature = "virtio-vsock"))]
const SYSNO_BIND: usize = 39;
#[cfg(any(feature = "net", feature = "virtio-vsock"))]
const SYSNO_LISTEN: usize = 40;
#[cfg(any(feature = "net", feature = "virtio-vsock"))]
const SYSNO_ACCEPT: usize = 41;
#[cfg(any(feature = "net", feature = "virtio-vsock"))]
const SYSNO_CONNECT: usize = 42;
#[cfg(any(feature = "net", feature = "virtio-vsock"))]
const SYSNO_RECV: usize = 43;
#[cfg(any(feature = "net", feature = "virtio-vsock"))]
const SYSNO_RECVFROM: usize = 44;
#[cfg(any(feature = "net", feature = "virtio-vsock"))]
const SYSNO_SEND: usize = 45;
#[cfg(any(feature = "net", feature = "virtio-vsock"))]
const SYSNO_SENDTO: usize = 46;
#[cfg(any(feature = "net", feature = "virtio-vsock"))]
const SYSNO_SHUTDOWN: usize = 47;
#[cfg(any(feature = "net", feature = "virtio-vsock"))]
const SYSNO_GETPEERNAME: usize = 48;
#[cfg(any(feature = "net", feature = "virtio-vsock"))]
const SYSNO_GETSOCKNAME: usize = 49;
#[cfg(any(feature = "net", feature = "virtio-vsock"))]
const SYSNO_GETSOCKOPT: usize = 50;
#[cfg(any(feature = "net", feature = "virtio-vsock"))]
const SYSNO_SETSOCKOPT: usize = 51;
#[cfg(any(feature = "net", feature = "virtio-vsock"))]
const SYSNO_GETADDRINFO: usize = 52;
#[cfg(any(feature = "net", feature = "virtio-vsock"))]
const SYSNO_FREEADDRINFO: usize = 53;
const SYSNO_AVAILABLE_PARALLELISM: usize = 54;
const SYSNO_GET_DENTS64: usize = 55;
const SYSNO_EXEC: usize = 56;
const SYSNO_MMAP: usize = 57;

/// Total number of system calls
pub(crate) const NO_SYSCALLS: usize = 64;

pub(crate) extern "C" fn invalid_syscall(sys_no: u64) -> ! {
	error!("Invalid syscall {sys_no}");
	sys_exit(1);
}

/// loader will replace this function
#[linkage = "weak"]
#[unsafe(no_mangle)]
pub extern "C" fn sys_spawn_process(
	_path: *const c_char,
	_argv: *const *const c_char,
	_envp: *const *const c_char,
) -> i32 {
	-i32::from(Errno::Nosys)
}

/// loader will replace this function
#[linkage = "weak"]
#[unsafe(no_mangle)]
pub extern "C" fn sys_exec(
	_path: *const c_char,
	_argv: *const *const c_char,
	_envp: *const *const c_char,
) -> i32 {
	-i32::from(Errno::Nosys)
}

#[allow(unused_assignments)]
#[unsafe(no_mangle)]
#[unsafe(naked)]
pub(crate) unsafe extern "C" fn sys_invalid() {
	naked_asm!(
		"mov rdi, rax",
		"call {}",
		sym invalid_syscall,
	);
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
		table.handle[SYSNO_USLEEP] = sys_usleep as *const _;
		table.handle[SYSNO_GETPID] = sys_getpid as *const _;
		table.handle[SYSNO_YIELD] = sys_yield as *const _;
		table.handle[SYSNO_READ_ENTROPY] = sys_read_entropy as *const _;
		table.handle[SYSNO_GET_PROCESSOR_COUNT] = sys_get_processor_count as *const _;
		table.handle[SYSNO_CLOSE] = sys_close as *const _;
		table.handle[SYSNO_FUTEX_WAIT] = sys_futex_wait as *const _;
		table.handle[SYSNO_FUTEX_WAKE] = sys_futex_wake as *const _;
		table.handle[SYSNO_OPEN] = sys_open as *const _;
		table.handle[SYSNO_READV] = sys_readv as *const _;
		table.handle[SYSNO_WRITEV] = sys_writev as *const _;
		table.handle[SYSNO_WAITPID] = sys_waitpid as *const _;
		table.handle[SYSNO_SPAWN_PROCESS] = sys_spawn_process as *const _;
		table.handle[SYSNO_CLOCK_GETTIME] = sys_clock_gettime as *const _;
		table.handle[SYSNO_SPAWN] = sys_spawn as *const _;
		table.handle[SYSNO_SPAWN2] = sys_spawn2 as *const _;
		table.handle[SYSNO_JOIN] = sys_join as *const _;
		table.handle[SYSNO_UNLINK] = sys_unlink as *const _;
		table.handle[SYSNO_MKDIR] = sys_mkdir as *const _;
		table.handle[SYSNO_RMDIR] = sys_rmdir as *const _;
		table.handle[SYSNO_STAT] = sys_stat as *const _;
		table.handle[SYSNO_LSTAT] = sys_lstat as *const _;
		table.handle[SYSNO_FSTAT] = sys_fstat as *const _;
		table.handle[SYSNO_DUP] = sys_dup as *const _;
		table.handle[SYSNO_IOCTL] = sys_ioctl as *const _;
		table.handle[SYSNO_POLL] = sys_poll as *const _;
		table.handle[SYSNO_BLOCK_CURRENT_TASK] = sys_block_current_task as *const _;
		table.handle[SYSNO_BLOCK_CURRENT_TASK_WITH_TIMEOUT] =
			sys_block_current_task_with_timeout as *const _;
		table.handle[SYSNO_WAKEUP_TASK] = sys_wakeup_task as *const _;
		#[cfg(any(feature = "net", feature = "virtio-vsock"))]
		{
			table.handle[SYSNO_SOCKET] = sys_socket as *const _;
			table.handle[SYSNO_BIND] = sys_bind as *const _;
			table.handle[SYSNO_LISTEN] = sys_listen as *const _;
			table.handle[SYSNO_ACCEPT] = sys_accept as *const _;
			table.handle[SYSNO_CONNECT] = sys_connect as *const _;
			table.handle[SYSNO_RECV] = sys_recv as *const _;
			table.handle[SYSNO_RECVFROM] = sys_recvfrom as *const _;
			table.handle[SYSNO_SEND] = sys_send as *const _;
			table.handle[SYSNO_SENDTO] = sys_sendto as *const _;
			table.handle[SYSNO_SHUTDOWN] = sys_shutdown as *const _;
			table.handle[SYSNO_GETPEERNAME] = sys_getpeername as *const _;
			table.handle[SYSNO_GETSOCKNAME] = sys_getsockname as *const _;
			table.handle[SYSNO_GETSOCKOPT] = sys_getsockopt as *const _;
			table.handle[SYSNO_SETSOCKOPT] = sys_setsockopt as *const _;
			table.handle[SYSNO_GETADDRINFO] = sys_getaddrinfo as *const _;
			table.handle[SYSNO_FREEADDRINFO] = sys_freeaddrinfo as *const _;
		}
		table.handle[SYSNO_AVAILABLE_PARALLELISM] = sys_available_parallelism as *const _;
		table.handle[SYSNO_GET_DENTS64] = sys_getdents64 as *const _;
		table.handle[SYSNO_EXEC] = sys_exec as *const _;
		table.handle[SYSNO_MMAP] = sys_mmap as *const _;

		table
	}
}

unsafe impl Send for SyscallTable {}
unsafe impl Sync for SyscallTable {}

#[unsafe(no_mangle)]
pub(crate) static SYSHANDLER_TABLE: SyscallTable = SyscallTable::new();
