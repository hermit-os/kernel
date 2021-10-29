use crate::environment;

extern "C" {
	static mut boot_gtod: u64;
}

pub fn get_boot_time() -> u64 {
	unsafe { boot_gtod }
}
