#[cfg(all(
	feature = "tcp",
	any(
		feature = "virtio-net",
		all(target_arch = "riscv64", feature = "gem-net", not(feature = "pci")),
		all(target_arch = "x86_64", feature = "rtl8139"),
	)
))]
pub(crate) mod tcp;
#[cfg(all(
	feature = "udp",
	any(
		feature = "virtio-net",
		all(target_arch = "riscv64", feature = "gem-net", not(feature = "pci")),
		all(target_arch = "x86_64", feature = "rtl8139"),
	)
))]
pub(crate) mod udp;
#[cfg(feature = "vsock")]
pub(crate) mod vsock;
