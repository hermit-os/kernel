#[cfg(feature = "pci")]
pub mod fuse;

pub fn init() {
	#[cfg(feature = "pci")]
	fuse::init();
}
