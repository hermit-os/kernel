// Copyright (c) 2018 Colin Finck, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

use arch;
use synch::spinlock::Spinlock;

static PARK_MILLER_LEHMER_SEED: Spinlock<u32> = Spinlock::new(0);

fn generate_park_miller_lehmer_random_number() -> u32 {
	let mut seed = PARK_MILLER_LEHMER_SEED.lock();
	let random = (((*seed) as u64 * 48271) % 2147483647) as u32;
	*seed = random;
	random
}

#[no_mangle]
pub extern "C" fn sys_rand() -> u32 {
	if let Some(value) = arch::processor::generate_random_number() {
		value
	} else {
		generate_park_miller_lehmer_random_number()
	}
}

pub fn random_init() {
	*PARK_MILLER_LEHMER_SEED.lock() = arch::processor::get_timestamp() as u32;
}

#[test]
fn random() {
	random_init();

	let  r = generate_park_miller_lehmer_random_number();
	assert!(r != sys_rand());
}