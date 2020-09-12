// Copyright (c) 2018 Colin Finck, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

use crate::arch;
use crate::synch::spinlock::Spinlock;

static PARK_MILLER_LEHMER_SEED: Spinlock<u32> = Spinlock::new(0);

fn generate_park_miller_lehmer_random_number() -> u32 {
	let mut seed = PARK_MILLER_LEHMER_SEED.lock();
	let random = ((u64::from(*seed) * 48271) % 2_147_483_647) as u32;
	*seed = random;
	random
}

fn __sys_rand32() -> Option<u32> {
	arch::processor::generate_random_number32()
}

fn __sys_rand64() -> Option<u64> {
	arch::processor::generate_random_number64()
}

fn __sys_rand() -> u32 {
	generate_park_miller_lehmer_random_number()
}

#[cfg(not(feature = "newlib"))]
#[no_mangle]
pub fn sys_secure_rand32() -> Option<u32> {
	kernel_function!(__sys_rand32())
}

#[cfg(not(feature = "newlib"))]
#[no_mangle]
pub fn sys_secure_rand64() -> Option<u64> {
	kernel_function!(__sys_rand64())
}

#[no_mangle]
pub extern "C" fn sys_rand() -> u32 {
	kernel_function!(__sys_rand())
}

fn __sys_srand(seed: u32) {
	*(PARK_MILLER_LEHMER_SEED.lock()) = seed;
}

#[no_mangle]
pub extern "C" fn sys_srand(seed: u32) {
	kernel_function!(__sys_srand(seed))
}

pub fn random_init() {
	let seed: u32 = arch::processor::get_timestamp() as u32;

	*PARK_MILLER_LEHMER_SEED.lock() = seed;
}
