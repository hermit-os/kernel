use crate::arch::get_processor_count;

/// Returns the number of processors currently online.
#[hermit_macro::system]
pub extern "C" fn sys_get_processor_count() -> usize {
	get_processor_count().try_into().unwrap()
}

/// Returns the processor frequency in MHz.
#[hermit_macro::system]
pub extern "C" fn sys_get_processor_frequency() -> u16 {
	crate::arch::processor::get_frequency()
}
