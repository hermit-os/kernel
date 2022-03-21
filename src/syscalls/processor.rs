use crate::arch::get_processor_count;

extern "C" fn __sys_get_processor_count() -> usize {
	get_processor_count().try_into().unwrap()
}

/// Returns the number of processors currently online.
#[no_mangle]
pub extern "C" fn sys_get_processor_count() -> usize {
	kernel_function!(__sys_get_processor_count())
}

extern "C" fn __sys_get_processor_frequency() -> u16 {
	crate::arch::processor::get_frequency()
}

/// Returns the processor frequency in MHz.
#[no_mangle]
pub extern "C" fn sys_get_processor_frequency() -> u16 {
	kernel_function!(__sys_get_processor_frequency())
}
