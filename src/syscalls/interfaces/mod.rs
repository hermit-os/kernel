use alloc::boxed::Box;
use alloc::vec::Vec;
use core::ffi::CStr;

#[cfg(all(not(feature = "pci"), not(target_arch = "aarch64")))]
use arch::kernel::mmio::get_network_driver;
#[cfg(all(feature = "pci", not(target_arch = "aarch64")))]
use arch::kernel::pci::get_network_driver;

pub use self::generic::*;
pub use self::uhyve::*;
use crate::errno::*;
use crate::syscalls::fs::{self};
use crate::{arch, env};

mod generic;
pub(crate) mod uhyve;

pub trait SyscallInterface: Send + Sync {
	fn init(&self) {
		// Interface-specific initialization steps.
	}

	fn get_application_parameters(&self) -> (i32, *const *const u8, *const *const u8) {
		let mut argv = Vec::new();

		let name = Box::leak(Box::new("{name}\0")).as_ptr();
		argv.push(name);

		let args = env::args();
		debug!("Setting argv as: {:?}", args);
		for arg in args {
			let ptr = Box::leak(format!("{arg}\0").into_boxed_str()).as_ptr();
			argv.push(ptr);
		}

		let mut envv = Vec::new();

		let envs = env::vars();
		debug!("Setting envv as: {:?}", envs);
		for (key, value) in envs {
			let ptr = Box::leak(format!("{key}={value}\0").into_boxed_str()).as_ptr();
			envv.push(ptr);
		}
		envv.push(core::ptr::null::<u8>());

		let argc = argv.len() as i32;
		let argv = Box::leak(argv.into_boxed_slice()).as_ptr();
		let envv = Box::leak(envv.into_boxed_slice()).as_ptr();

		(argc, argv, envv)
	}

	fn shutdown(&self, _arg: i32) -> ! {
		arch::processor::shutdown()
	}

	fn get_mac_address(&self) -> Result<[u8; 6], ()> {
		#[cfg(not(target_arch = "aarch64"))]
		match get_network_driver() {
			Some(driver) => Ok(driver.lock().get_mac_address()),
			_ => Err(()),
		}
		#[cfg(target_arch = "aarch64")]
		Err(())
	}

	fn get_mtu(&self) -> Result<u16, ()> {
		#[cfg(not(target_arch = "aarch64"))]
		match get_network_driver() {
			Some(driver) => Ok(driver.lock().get_mtu()),
			_ => Err(()),
		}
		#[cfg(target_arch = "aarch64")]
		Err(())
	}

	fn has_packet(&self) -> bool {
		#[cfg(not(target_arch = "aarch64"))]
		match get_network_driver() {
			Some(driver) => driver.lock().has_packet(),
			_ => false,
		}
		#[cfg(target_arch = "aarch64")]
		false
	}

	fn get_tx_buffer(&self, len: usize) -> Result<(*mut u8, usize), ()> {
		#[cfg(not(target_arch = "aarch64"))]
		match get_network_driver() {
			Some(driver) => driver.lock().get_tx_buffer(len),
			_ => Err(()),
		}
		#[cfg(target_arch = "aarch64")]
		Err(())
	}

	fn free_tx_buffer(&self, handle: usize) -> Result<(), ()> {
		#[cfg(not(target_arch = "aarch64"))]
		match get_network_driver() {
			Some(driver) => {
				driver.lock().free_tx_buffer(handle);
				Ok(())
			}
			_ => Err(()),
		}
		#[cfg(target_arch = "aarch64")]
		Err(())
	}

	fn send_tx_buffer(&self, handle: usize, len: usize) -> Result<(), ()> {
		#[cfg(not(target_arch = "aarch64"))]
		match get_network_driver() {
			Some(driver) => driver.lock().send_tx_buffer(handle, len),
			_ => Err(()),
		}
		#[cfg(target_arch = "aarch64")]
		Err(())
	}

	fn receive_rx_buffer(&self) -> Result<Vec<u8>, ()> {
		#[cfg(not(target_arch = "aarch64"))]
		match get_network_driver() {
			Some(driver) => driver.lock().receive_rx_buffer(),
			_ => Err(()),
		}
		#[cfg(target_arch = "aarch64")]
		Err(())
	}

	#[cfg(not(target_arch = "x86_64"))]
	fn unlink(&self, _name: *const u8) -> i32 {
		debug!("unlink is unimplemented, returning -ENOSYS");
		-ENOSYS
	}

	#[cfg(target_arch = "x86_64")]
	fn unlink(&self, name: *const u8) -> i32 {
		let name = unsafe { CStr::from_ptr(name as _) }.to_str().unwrap();
		debug!("unlink {}", name);

		fs::FILESYSTEM
			.lock()
			.unlink(name)
			.expect("Unlinking failed!"); // TODO: error handling
		0
	}

	fn stat(&self, _file: *const u8, _st: usize) -> i32 {
		info!("stat is unimplemented");
		-ENOSYS
	}
}
