cfg_if::cfg_if! {
	if #[cfg(target_os = "none")] {
		mod none;
		pub use self::none::*;
	} else if #[cfg(target_os = "uefi")] {
		mod uefi;
		pub use self::uefi::*;
	}
}
