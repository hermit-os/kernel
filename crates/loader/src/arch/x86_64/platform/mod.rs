cfg_if::cfg_if! {
	if #[cfg(feature = "linux")] {
		mod linux;
		pub use self::linux::*;
	} else if #[cfg(feature = "multiboot")] {
		mod multiboot;
		pub use self::multiboot::*;
	}
}
