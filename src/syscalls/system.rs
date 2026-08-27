use crate::arch::mm::paging::{BasePageSize, PageSize};

/// Returns the base page size, in bytes, of the current system.
#[hermit_macro::system]
#[unsafe(no_mangle)]
pub extern "C" fn sys_getpagesize() -> i32 {
	BasePageSize::SIZE.try_into().unwrap()
}

// Writes the scancodes from the keyboard buffer into the provided buffer.
// If 'nonblock' is true, it will return immediately if there are no scancodes available,
// otherwise it will block until at least one scancode is available.
// Returns the number of bytes written to the buffer,
// or a negative error code on failure.
#[cfg(all(target_arch = "x86_64", feature = "pc-keyboard"))]
#[hermit_macro::system]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_read_keyboard(buffer: *mut u8, size: usize, nonblock: bool) -> isize {
	if buffer.is_null() {
		return -(crate::errno::Errno::Fault as isize);
	}
	if size == 0 {
		return 0;
	}
	// SAFETY: We have to trust the user input, because we are a unikernel and if the user wants to crash the program
	// they are free to do so.
	let buffer_slice: &mut [u8] = unsafe { core::slice::from_raw_parts_mut(buffer, size) };
	let result = crate::arch::kernel::pc_keyboard::pop_scancodes(buffer_slice, nonblock);
	if result == 0 && nonblock {
		-(crate::errno::Errno::Again as isize)
	} else {
		result as isize
	}
}
/// Returns the number of bytes written to the resume_params buffer.
#[hermit_macro::system]
#[unsafe(no_mangle)]
#[cfg(all(feature = "snapshot", feature = "uhyve"))]
pub unsafe extern "C" fn sys_snapshot(resume_params: *mut u8, len: u64) -> u64 {
	use core::ffi::CStr;
	#[cfg(feature = "net")]
	use core::str::FromStr;

	use uhyve_interface::GuestPhysAddr;
	use uhyve_interface::v2::Hypercall;
	use uhyve_interface::v2::parameters::SnapshotParams;

	use crate::alloc::string::ToString;
	use crate::env::{self, UhyveStartInfo, insert_var};
	#[cfg(feature = "net")]
	use crate::executor::network;
	use crate::uhyve::uhyve_hypercall;

	assert!(env::start_info().is_uhyve());
	let new_args = if resume_params.is_null() {
		GuestPhysAddr::zero()
	} else {
		GuestPhysAddr::new(
			crate::arch::mm::paging::virtual_to_physical(crate::mm::VirtAddr::from_ptr(
				resume_params,
			))
			.unwrap()
			.as_u64(),
		)
	};
	let mut snapshot_params = SnapshotParams {
		new_args,
		new_args_len: len,
		..Default::default()
	};
	uhyve_hypercall(Hypercall::Snapshot(&mut snapshot_params));

	if snapshot_params.restored {
		if let Some(ip) = snapshot_params.new_hermit_ip {
			let ip = CStr::from_bytes_until_nul(&ip)
				.expect("The hypervisor supplied an unterminated HERMIT_IP")
				.to_str()
				.expect("The hypervisor supplied a non-UTF-8 HERMIT_IP");
			insert_var("HERMIT_IP", ip.to_string());
		};
		#[cfg(feature = "net")]
		network::reinit();
	}
	snapshot_params.new_args_len
}
