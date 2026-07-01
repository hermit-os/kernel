#[macro_use]
mod arch;

use core::ffi::{c_char, c_int, c_void};
use core::fmt::{self, Write};

#[unsafe(no_mangle)]
unsafe extern "C" fn _start(_argc: c_int, _argv: *mut *mut c_char) -> ! {
	use alloc::borrow::ToOwned;
	use alloc::vec;
	use core::ptr;

	let (argv, argv_len, _args_cap) =
		vec![c"dummy".to_owned().into_raw(), ptr::null_mut()].into_raw_parts();
	let argc = argv_len - 1;
	let argc = i32::try_from(argc).unwrap();

	let envp = ptr::null_mut();

	unsafe extern "C" {
		fn runtime_entry(argc: c_int, argv: *mut *mut c_char, envp: *mut *mut c_char) -> !;
	}

	unsafe { runtime_entry(argc, argv, envp) }
}

#[unsafe(no_mangle)]
#[linkage = "weak"]
extern "C" fn sys_errno_location() -> *mut i32 {
	use core::cell::UnsafeCell;

	#[thread_local]
	static ERRNO: UnsafeCell<i32> = UnsafeCell::new(0);

	ERRNO.get()
}

#[unsafe(no_mangle)]
pub extern "C" fn sys_get_errno() -> c_int {
	unsafe { *sys_errno_location() }
}

#[unsafe(no_mangle)]
pub extern "C" fn sys_errno() -> i32 {
	unsafe { *sys_errno_location() }
}

macro_rules! export {
	() => ();

	(fn $fn:ident($($arg:ident: $argty:ty),*); $($rest:tt)*) => {
		#[unsafe(no_mangle)]
		unsafe extern "C" fn ${concat(sys_, $fn)}($($arg: $argty),*) {
			unsafe {
				syscall!(SyscallNo::$fn, $($arg),*);
			}
		}

		export!($($rest)*);
	};

	(fn $fn:ident($($arg:ident: $argty:ty),*) -> !; $($rest:tt)*) => {
		#[unsafe(no_mangle)]
		unsafe extern "C" fn ${concat(sys_, $fn)}($($arg: $argty),*) -> ! {
			unsafe {
				syscall!(SyscallNo::$fn, $($arg),*);
			}

			unreachable!()
		}

		export!($($rest)*);
	};

	(fn $fn:ident($($arg:ident: $argty:ty),*) -> $retty:ty; $($rest:tt)*) => {
		#[unsafe(no_mangle)]
		unsafe extern "C" fn ${concat(sys_, $fn)}($($arg: $argty),*) -> $retty {
			let r0 = unsafe { syscall!(SyscallNo::$fn, $($arg),*) };
			let r0 = r0 as $retty;

			if r0 < 0 {
				let errno = c_int::try_from(-r0).unwrap();
				// SAFETY: ERRNO is thread-local
				unsafe {
					*sys_errno_location() = errno;
				}
			}

			r0
		}

		export!($($rest)*);
	};

	(#[no_errno] fn $fn:ident($($arg:ident: $argty:ty),*) -> $retty:ty; $($rest:tt)*) => {
		#[unsafe(no_mangle)]
		unsafe extern "C" fn ${concat(sys_, $fn)}($($arg: $argty),*) -> $retty {
			let r0 = unsafe { syscall!(SyscallNo::$fn, $($arg),*) };
			r0 as $retty
		}

		export!($($rest)*);
	};
}

export! {
	fn exit(arg: i32) -> !;
	fn read(fd: i32, buf: *mut u8, len: usize) -> isize;
	fn write(fd: i32, buf: *const u8, len: usize) -> isize;
	fn usleep(usecs: u64);
	#[no_errno] fn getpid() -> u32;
	// fn sys_yield();
	fn read_entropy(buf: *mut u8, len: usize, flags: u32) -> isize;
	#[no_errno] fn get_processor_count() -> usize;
	fn close(fd: i32) -> i32;
	fn futex_wait(address: *mut u32, expected: u32, timeout: *const libc::timespec, flags: u32) -> i32;
	fn futex_wake(address: *mut u32, count: i32) -> i32;
	fn open(name: *const i8, flags: i32, mode: i32) -> i32;
	fn writev(fd: i32, iov: *const u8, iovcnt: usize) -> isize;
	fn readv(fd: i32, iov: *const u8, iovcnt: usize) -> isize;
	fn fork() -> libc::pid_t;
	fn waitpid(pid: libc::pid_t) -> i32;
	fn spawn_process(path: *const c_char) -> libc::pid_t;
	fn clock_gettime(clock_id: u64, tp: *mut libc::timespec) -> i32;
	fn spawn(id: *mut libc::Tid, func: extern "C" fn(usize), arg: usize, prio: u8, core_id: isize) -> i32;
	#[no_errno] fn spawn2(func: extern "C" fn(usize), arg: usize, prio: u8, stack_size: usize, core_id: isize) -> libc::Tid;
	fn join(id: libc::Tid) -> i32;
	fn unlink(name: *const i8) -> i32;
	fn mkdir(name: *const i8, mode: u32) -> i32;
	fn rmdir(name: *const i8) -> i32;
	fn stat(name: *const i8, stat: *mut libc::stat) -> i32;
	fn lstat(name: *const i8, stat: *mut libc::stat) -> i32;
	fn fstat(fd: i32, stat: *mut libc::stat) -> i32;
	fn dup(fd: i32) -> i32;
	fn ioctl(s: i32, cmd: i32, argp: *mut c_void) -> i32;
	fn poll(fds: *mut libc::pollfd, nfds: libc::nfds_t, timeout: i32) -> i32;
	fn notify(id: usize, count: i32) -> i32;
	fn add_queue(id: usize, timeout_ns: i64) -> i32;
	fn wait(id: usize) -> i32;
	fn init_queue(id: usize) -> i32;
	fn destroy_queue(id: usize) -> i32;
	fn block_current_task();
	fn block_current_task_with_timeout(timeout: u64);
	fn wakeup_task(tid: libc::Tid);
	fn socket(domain: i32, type_: i32, protocol: i32) -> i32;
	fn bind(s: i32, name: *const libc::sockaddr, namelen: libc::socklen_t) -> i32;
	fn listen(s: i32, backlog: i32) -> i32;
	fn accept(s: i32, addr: *mut libc::sockaddr, addrlen: *mut libc::socklen_t) -> i32;
	fn connect(s: i32, name: *const libc::sockaddr, namelen: libc::socklen_t) -> i32;
	fn recv(socket: i32, buf: *mut u8, len: usize, flags: i32) -> isize;
	fn recvfrom(socket: i32, buf: *mut u8, len: usize, flags: i32, addr: *mut libc::sockaddr, addrlen: *mut libc::socklen_t) -> isize;
	fn send(s: i32, mem: *const c_void, len: usize, flags: i32) -> isize;
	fn sendto(s: i32, mem: *const c_void, len: usize, flags: i32, addr: *const libc::sockaddr, addr_len: libc::socklen_t) -> isize;
	fn shutdown(s: i32, how: i32) -> i32;
	fn getpeername(s: i32, name: *mut libc::sockaddr, namelen: *mut libc::socklen_t) -> i32;
	fn getsockname(s: i32, name: *mut libc::sockaddr, namelen: *mut libc::socklen_t) -> i32;
	fn getsockopt(s: i32, level: i32, optname: i32, optval: *mut c_void, optlen: *mut libc::socklen_t) -> i32;
	fn setsockopt(s: i32, level: i32, optname: i32, optval: *const c_void, optlen: libc::socklen_t) -> i32;
	fn getaddrinfo(nodename: *const i8, servname: *const i8, hints: *const libc::addrinfo, res: *mut *mut libc::addrinfo) -> i32;
	fn freeaddrinfo(ai: *mut libc::addrinfo);
	#[no_errno] fn available_parallelism() -> usize;
	fn getdents64(fd: i32, dirp: *mut libc::dirent64, count: usize) -> i64;
	fn exec(path: *const c_char) -> i32;
	fn mmap(size: usize, prot_flags: u32, ret: *mut *mut u8) -> i32;
	fn isatty(fd: i32) -> i32;
	#[no_errno] fn getcwd(buf: *mut c_char, size: usize) -> *const c_char;
	#[no_errno] fn getpagesize() -> i32;
	fn mlock(addr: *const c_void, size: usize) -> i32;
	fn mlockall(flags: c_int) -> i32;
	fn munlock(addr: *const c_void, size: usize) -> i32;
	fn munlockall(flags: c_int) -> i32;
	fn fchmod(fd: i32, mode: u32) -> i32;
	fn gettimeofday(tp: *mut libc::timeval, tz: usize) -> i32;
	fn faccessat(dirfd: i32, name: *const c_char, _mode: i32, flags: i32) -> i32 ;
	fn clock_getres(clock_id: libc::clockid_t, res: *mut libc::timespec) -> i32;
	fn clock_settime(clock_id: libc::clockid_t, tp: *const libc::timespec) -> i32;
	fn nanosleep(rqtp: *const libc::timespec, rmtp: *mut libc::timespec) -> i32;
	fn access(name: *const c_char, flags: i32) -> i32;
	fn chdir(path: *mut c_char) -> i32;
	fn dup2(fd1: i32, fd2: i32) -> i32;
	fn fchdir(fd: i32) -> i32;
	fn lseek(fd: i32, offset: isize, whence: i32) -> isize;
	fn truncate(path: *const c_char, size: usize) -> i32;
	fn ftruncate(fd: i32, size: usize) -> i32;
	fn fcntl(fd: i32, cmd: i32, arg: i32) -> i32;
}

#[unsafe(no_mangle)]
extern "C" fn sys_yield() {
	unsafe {
		syscall!(SyscallNo::r#yield);
	}
}

#[unsafe(no_mangle)]
extern "C" fn sys_abort() -> ! {
	unsafe { sys_exit(1) }
}

#[derive(PartialEq, Eq, Debug)]
#[allow(non_camel_case_types)]
#[repr(usize)]
enum SyscallNo {
	exit = 0,
	write = 1,
	read = 2,
	usleep = 3,
	getpid = 4,
	r#yield = 5,
	read_entropy = 6,
	get_processor_count = 7,
	close = 8,
	futex_wait = 9,
	futex_wake = 10,
	open = 11,
	writev = 12,
	readv = 13,
	fork = 14,
	waitpid = 15,
	spawn_process = 16,
	clock_gettime = 17,
	spawn = 18,
	spawn2 = 19,
	join = 20,
	unlink = 21,
	mkdir = 22,
	rmdir = 23,
	stat = 24,
	lstat = 25,
	fstat = 26,
	dup = 27,
	ioctl = 28,
	poll = 29,
	notify = 30,
	add_queue = 31,
	wait = 32,
	init_queue = 33,
	destroy_queue = 34,
	block_current_task = 35,
	block_current_task_with_timeout = 36,
	wakeup_task = 37,
	socket = 38,
	bind = 39,
	listen = 40,
	accept = 41,
	connect = 42,
	recv = 43,
	recvfrom = 44,
	send = 45,
	sendto = 46,
	shutdown = 47,
	getpeername = 48,
	getsockname = 49,
	getsockopt = 50,
	setsockopt = 51,
	getaddrinfo = 52,
	freeaddrinfo = 53,
	available_parallelism = 54,
	getdents64 = 55,
	exec = 56,
	mmap = 57,
	isatty = 58,
	getcwd = 59,
	getpagesize = 60,
	mlock = 61,
	mlockall = 62,
	munlock = 63,
	munlockall = 64,
	fchmod = 65,
	gettimeofday = 66,
	faccessat = 67,
	clock_getres = 68,
	clock_settime = 69,
	nanosleep = 70,
	access = 71,
	chdir = 72,
	dup2 = 73,
	fchdir = 74,
	lseek = 75,
	truncate = 76,
	ftruncate = 77,
	fcntl = 78,
}

struct Stderr;

impl fmt::Write for Stderr {
	fn write_str(&mut self, s: &str) -> fmt::Result {
		let n = unsafe { sys_write(libc::STDOUT_FILENO, s.as_ptr().cast(), s.len()) };

		if n != s.len() as isize {
			return Err(Default::default());
		}

		Ok(())
	}
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
	writeln!(Stderr, "{info}").ok();

	loop {}
}
