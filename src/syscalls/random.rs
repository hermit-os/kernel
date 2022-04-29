use crate::arch;
use crate::synch::spinlock::Spinlock;

static PARK_MILLER_LEHMER_SEED: Spinlock<u32> = Spinlock::new(0);
const RAND_MAX: u64 = 2_147_483_647;

fn generate_park_miller_lehmer_random_number() -> u32 {
	let mut seed = PARK_MILLER_LEHMER_SEED.lock();
	let random = ((u64::from(*seed) * 48271) % RAND_MAX) as u32;
	*seed = random;
	random
}

unsafe extern "C" fn __sys_rand32(value: *mut u32) -> i32 {
	let rand = try_sys!(arch::processor::generate_random_number32().ok_or("sys_rand32 failed"));
	unsafe {
		value.write(rand);
	}
	0
}

unsafe extern "C" fn __sys_rand64(value: *mut u64) -> i32 {
	let rand = try_sys!(arch::processor::generate_random_number64().ok_or("sys_rand64 failed"));
	unsafe {
		value.write(rand);
	}
	0
}

extern "C" fn __sys_rand() -> u32 {
	generate_park_miller_lehmer_random_number()
}

/// Create a cryptographicly secure 32bit random number with the support of
/// the underlying hardware. If the required hardware isn't available,
/// the function returns `None`.
#[cfg(not(feature = "newlib"))]
#[no_mangle]
pub unsafe extern "C" fn sys_secure_rand32(value: *mut u32) -> i32 {
	kernel_function!(__sys_rand32(value))
}

/// Create a cryptographicly secure 64bit random number with the support of
/// the underlying hardware. If the required hardware isn't available,
/// the function returns `None`.
#[cfg(not(feature = "newlib"))]
#[no_mangle]
pub unsafe extern "C" fn sys_secure_rand64(value: *mut u64) -> i32 {
	kernel_function!(__sys_rand64(value))
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

pub(crate) fn random_init() {
	let seed: u32 = arch::processor::get_timestamp() as u32;

	*PARK_MILLER_LEHMER_SEED.lock() = seed;
}
