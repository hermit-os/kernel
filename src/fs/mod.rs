#[cfg(all(feature = "pci"))]
pub mod fuse;

pub fn init() {
	#[cfg(all(feature = "pci"))]
	fuse::init();
}
