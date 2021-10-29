use crate::scheduler::CoreId;

pub fn set_oneshot_timer(wakeup_time: Option<u64>) {
	// TODO
	debug!("set_oneshot_timer stub");
}

pub fn wakeup_core(core_to_wakeup: CoreId) {
	// TODO
	debug!("wakeup_core stub");
}

#[no_mangle]
pub extern "C" fn do_bad_mode() {}

#[no_mangle]
pub extern "C" fn do_error() {}

#[no_mangle]
pub extern "C" fn do_fiq() {}

#[no_mangle]
pub extern "C" fn do_irq() {}

#[no_mangle]
pub extern "C" fn do_sync() {}

#[no_mangle]
pub extern "C" fn eoi() {}

#[no_mangle]
pub extern "C" fn finish_task_switch() {}

#[no_mangle]
pub extern "C" fn getcontext() {}

#[no_mangle]
pub extern "C" fn get_current_stack() {}

#[no_mangle]
pub extern "C" fn makecontext() {}

#[no_mangle]
pub extern "C" fn setcontext() {}
