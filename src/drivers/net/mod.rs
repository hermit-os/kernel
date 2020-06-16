// Copyright (c) 2019 Stefan Lankes, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

use crate::synch::semaphore::*;

static NET_SEM: Semaphore = Semaphore::new(0);

pub fn netwakeup() {
	NET_SEM.release();
}

pub fn netwait(millis: Option<u64>) {
	match millis {
		Some(ms) => {
			if ms > 0 {
				NET_SEM.acquire(Some(ms));
			} else {
				NET_SEM.try_acquire();
			}
		}
		_ => {
			NET_SEM.acquire(None);
		}
	};
}
