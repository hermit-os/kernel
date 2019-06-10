// Copyright (c) 2019 Stefan Lankes, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

pub mod uhyve;

use synch::semaphore::*;
use scheduler::task::TaskId;

static NET_SEM: Semaphore = Semaphore::new(0);
static mut NETWORK_TASK_ID: TaskId = TaskId::from(0);

pub fn get_network_task_id() -> TaskId {
	unsafe {
		NETWORK_TASK_ID
	}
}