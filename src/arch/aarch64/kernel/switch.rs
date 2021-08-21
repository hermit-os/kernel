#[no_mangle]
pub extern "C" fn switch_to_fpu_owner(_old_stack: *mut usize, _new_stack: usize) {}

#[no_mangle]
pub extern "C" fn switch_to_task(_old_stack: *mut usize, _new_stack: usize) {}
