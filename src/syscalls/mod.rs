#![allow(clippy::result_unit_err)]

#[cfg(all(not(feature = "newlib"), target_arch = "x86_64"))]
use crate::drivers::net::*;
use crate::env;
#[cfg(feature = "newlib")]
use crate::synch::spinlock::SpinlockIrqSave;
use crate::syscalls::interfaces::SyscallInterface;
#[cfg(target_os = "none")]
use crate::{__sys_free, __sys_malloc, __sys_realloc};

pub use self::condvar::*;
pub use self::processor::*;
pub use self::random::*;
pub use self::recmutex::*;
pub use self::semaphore::*;
pub use self::spinlock::*;
pub use self::system::*;
pub use self::tasks::*;
pub use self::timer::*;

mod condvar;
pub(crate) mod fs;
mod interfaces;
#[cfg(feature = "newlib")]
mod lwip;
mod processor;
mod random;
mod recmutex;
mod semaphore;
mod spinlock;
mod system;
mod tasks;
mod timer;

#[cfg(feature = "newlib")]
const LWIP_FD_BIT: i32 = 1 << 30;

#[cfg(feature = "newlib")]
pub static LWIP_LOCK: SpinlockIrqSave<()> = SpinlockIrqSave::new(());

static mut SYS: &'static dyn SyscallInterface = &interfaces::Generic;

/// Shuts down the machine.
///
/// This does not require the syscall interface to be initialized.
pub(crate) fn shutdown(arg: i32) -> ! {
	if env::is_uhyve() {
		interfaces::Uhyve.shutdown(arg)
	} else {
		interfaces::Generic.shutdown(arg)
	}
}

pub(crate) fn init() {
	unsafe {
		// We know that HermitCore has successfully initialized a network interface.
		// Now check if we can load a more specific SyscallInterface to make use of networking.
		if env::is_proxy() {
			panic!("Currently, we don't support the proxy mode!");
		} else if env::is_uhyve() {
			SYS = &interfaces::Uhyve;
		}

		// Perform interface-specific initialization steps.
		SYS.init();
	}

	random_init();
	#[cfg(feature = "newlib")]
	sbrk_init();
}

#[cfg(target_os = "none")]
#[no_mangle]
pub extern "C" fn sys_malloc(size: usize, align: usize) -> *mut u8 {
	__sys_malloc(size, align)
}

#[cfg(target_os = "none")]
#[no_mangle]
pub extern "C" fn sys_realloc(ptr: *mut u8, size: usize, align: usize, new_size: usize) -> *mut u8 {
	__sys_realloc(ptr, size, align, new_size)
}

#[cfg(target_os = "none")]
#[no_mangle]
pub extern "C" fn sys_free(ptr: *mut u8, size: usize, align: usize) {
	__sys_free(ptr, size, align)
}

pub(crate) fn get_application_parameters() -> (i32, *const *const u8, *const *const u8) {
	unsafe { SYS.get_application_parameters() }
}

#[allow(improper_ctypes_definitions)]
extern "C" fn __sys_get_mac_address() -> Result<[u8; 6], ()> {
	unsafe { SYS.get_mac_address() }
}

#[no_mangle]
pub fn sys_get_mac_address() -> Result<[u8; 6], ()> {
	kernel_function!(__sys_get_mac_address())
}

#[allow(improper_ctypes_definitions)]
extern "C" fn __sys_get_mtu() -> Result<u16, ()> {
	unsafe { SYS.get_mtu() }
}

#[no_mangle]
pub fn sys_get_mtu() -> Result<u16, ()> {
	kernel_function!(__sys_get_mtu())
}

extern "C" fn __sys_get_tx_buffer(len: usize, ret: &mut Result<(*mut u8, usize), ()>) {
	*ret = unsafe { SYS.get_tx_buffer(len) };
}

#[allow(improper_ctypes_definitions)]
extern "C" fn __sys_free_tx_buffer(handle: usize) -> Result<(), ()> {
	unsafe { SYS.free_tx_buffer(handle) }
}

#[no_mangle]
pub fn sys_free_tx_buffer(handle: usize) -> Result<(), ()> {
	kernel_function!(__sys_free_tx_buffer(handle))
}

#[no_mangle]
pub fn sys_get_tx_buffer(len: usize) -> Result<(*mut u8, usize), ()> {
	let mut ret = Err(());
	kernel_function!(__sys_get_tx_buffer(len, &mut ret));
	ret
}

#[allow(improper_ctypes_definitions)]
extern "C" fn __sys_send_tx_buffer(handle: usize, len: usize) -> Result<(), ()> {
	unsafe { SYS.send_tx_buffer(handle, len) }
}

#[no_mangle]
pub fn sys_send_tx_buffer(handle: usize, len: usize) -> Result<(), ()> {
	kernel_function!(__sys_send_tx_buffer(handle, len))
}

extern "C" fn __sys_receive_rx_buffer(ret: &mut Result<(&'static [u8], usize), ()>) {
	*ret = unsafe { SYS.receive_rx_buffer() };
}

#[no_mangle]
pub fn sys_receive_rx_buffer() -> Result<(&'static [u8], usize), ()> {
	let mut ret = Err(());
	kernel_function!(__sys_receive_rx_buffer(&mut ret));
	ret
}

#[allow(improper_ctypes_definitions)]
extern "C" fn __sys_rx_buffer_consumed(handle: usize) -> Result<(), ()> {
	unsafe { SYS.rx_buffer_consumed(handle) }
}

#[no_mangle]
pub fn sys_rx_buffer_consumed(handle: usize) -> Result<(), ()> {
	kernel_function!(__sys_rx_buffer_consumed(handle))
}

#[cfg(all(not(feature = "newlib"), target_arch = "x86_64"))]
#[no_mangle]
pub extern "C" fn sys_netwait() {
	kernel_function!(netwait());
}

#[cfg(all(not(feature = "newlib"), target_arch = "x86_64"))]
#[no_mangle]
pub extern "C" fn sys_set_network_polling_mode(value: bool) {
	kernel_function!(set_polling_mode(value));
}

pub(crate) extern "C" fn __sys_shutdown(arg: i32) -> ! {
	// print some performance statistics
	crate::arch::kernel::print_statistics();

	unsafe { SYS.shutdown(arg) }
}

#[no_mangle]
pub extern "C" fn sys_shutdown(arg: i32) -> ! {
	kernel_function!(__sys_shutdown(arg))
}

extern "C" fn __sys_unlink(name: *const u8) -> i32 {
	unsafe { SYS.unlink(name) }
}

#[no_mangle]
pub extern "C" fn sys_unlink(name: *const u8) -> i32 {
	kernel_function!(__sys_unlink(name))
}

extern "C" fn __sys_open(name: *const u8, flags: i32, mode: i32) -> i32 {
	unsafe { SYS.open(name, flags, mode) }
}

#[no_mangle]
pub extern "C" fn sys_open(name: *const u8, flags: i32, mode: i32) -> i32 {
	kernel_function!(__sys_open(name, flags, mode))
}

extern "C" fn __sys_close(fd: i32) -> i32 {
	unsafe { SYS.close(fd) }
}

#[no_mangle]
pub extern "C" fn sys_close(fd: i32) -> i32 {
	kernel_function!(__sys_close(fd))
}

extern "C" fn __sys_read(fd: i32, buf: *mut u8, len: usize) -> isize {
	unsafe { SYS.read(fd, buf, len) }
}
#[no_mangle]
pub extern "C" fn sys_read(fd: i32, buf: *mut u8, len: usize) -> isize {
	kernel_function!(__sys_read(fd, buf, len))
}

extern "C" fn __sys_write(fd: i32, buf: *const u8, len: usize) -> isize {
	unsafe { SYS.write(fd, buf, len) }
}

#[no_mangle]
pub extern "C" fn sys_write(fd: i32, buf: *const u8, len: usize) -> isize {
	kernel_function!(__sys_write(fd, buf, len))
}

extern "C" fn __sys_lseek(fd: i32, offset: isize, whence: i32) -> isize {
	unsafe { SYS.lseek(fd, offset, whence) }
}

#[no_mangle]
pub extern "C" fn sys_lseek(fd: i32, offset: isize, whence: i32) -> isize {
	kernel_function!(__sys_lseek(fd, offset, whence))
}

extern "C" fn __sys_stat(file: *const u8, st: usize) -> i32 {
	unsafe { SYS.stat(file, st) }
}

#[no_mangle]
pub extern "C" fn sys_stat(file: *const u8, st: usize) -> i32 {
	kernel_function!(__sys_stat(file, st))
}
