#[cfg(all(feature = "pci", target_arch = "x86_64"))]
pub mod fuse;

pub fn init() {
	#[cfg(all(feature = "pci", target_arch = "x86_64"))]
	fuse::init();
}
