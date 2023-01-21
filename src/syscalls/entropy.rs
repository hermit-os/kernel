use core::mem::size_of;
use core::slice;

use hermit_sync::TicketMutex;

use crate::arch;
use crate::entropy::{self, Flags};
use crate::errno::EINVAL;

static PARK_MILLER_LEHMER_SEED: TicketMutex<u32> = TicketMutex::new(0);
const RAND_MAX: u64 = 2_147_483_647;

fn generate_park_miller_lehmer_random_number() -> u32 {
	let mut seed = PARK_MILLER_LEHMER_SEED.lock();
	let random = ((u64::from(*seed) * 48271) % RAND_MAX) as u32;
	*seed = random;
	random
}

unsafe extern "C" fn __sys_read_entropy(buf: *mut u8, len: usize, flags: u32) -> i32 {
	let Some(flags) = Flags::from_bits(flags) else { return -EINVAL };

	if len > isize::MAX as usize {
		return -EINVAL;
	}

	let buf = unsafe {
		buf.write_bytes(0, len);
		slice::from_raw_parts_mut(buf, len)
	};

	entropy::read(buf, flags)
}

/// Fill `len` bytes in `buf` with cryptographically secure random data.
///
/// Returns
/// * `-EINVAL` if `flags` contains unknown flags.
/// * `-EINVAL` if `len` is larger than `isize::MAX`.
/// * `-ENOSYS` if the system does not support random data generation.
#[no_mangle]
pub unsafe extern "C" fn sys_read_entropy(buf: *mut u8, len: usize, flags: u32) -> i32 {
	kernel_function!(__sys_read_entropy(buf, len, flags))
}

/// Create a cryptographicly secure 32bit random number with the support of
/// the underlying hardware. If the required hardware isn't available,
/// the function returns `None`.
#[cfg(not(feature = "newlib"))]
#[no_mangle]
pub unsafe extern "C" fn sys_secure_rand32(value: *mut u32) -> i32 {
	unsafe { sys_read_entropy(value.cast(), size_of::<u32>(), 0) }
}

/// Create a cryptographicly secure 64bit random number with the support of
/// the underlying hardware. If the required hardware isn't available,
/// the function returns `None`.
#[cfg(not(feature = "newlib"))]
#[no_mangle]
pub unsafe extern "C" fn sys_secure_rand64(value: *mut u64) -> i32 {
	unsafe { sys_read_entropy(value.cast(), size_of::<u64>(), 0) }
}

extern "C" fn __sys_rand() -> u32 {
	generate_park_miller_lehmer_random_number()
}

/// The function computes a sequence of pseudo-random integers
/// in the range of 0 to RAND_MAX
#[no_mangle]
pub extern "C" fn sys_rand() -> u32 {
	kernel_function!(__sys_rand())
}

extern "C" fn __sys_srand(seed: u32) {
	*(PARK_MILLER_LEHMER_SEED.lock()) = seed;
}

/// The function sets its argument as the seed for a new sequence
/// of pseudo-random numbers to be returned by rand()
#[no_mangle]
pub extern "C" fn sys_srand(seed: u32) {
	kernel_function!(__sys_srand(seed))
}

pub(crate) fn init_entropy() {
	let seed: u32 = arch::processor::get_timestamp() as u32;

	*PARK_MILLER_LEHMER_SEED.lock() = seed;
}
