use core::num::NonZero;

#[cfg(not(any(target_arch = "aarch64", target_arch = "riscv64")))]
pub fn start_info() -> &'static impl super::StartInfo {
	#[expect(unreachable_code)]
	&panic!()
}

#[cfg(any(target_arch = "aarch64", target_arch = "riscv64"))]
pub fn start_info() -> &'static impl super::FdtStartInfo {
	#[expect(unreachable_code)]
	&panic!()
}

impl super::StartInfo for ! {
	fn bootargs(&self) -> Option<&str> {
		*self
	}

	fn rsdp_addr(&self) -> Option<NonZero<usize>> {
		*self
	}
}

#[cfg(any(target_arch = "aarch64", target_arch = "riscv64"))]
impl super::FdtStartInfo for ! {
	fn fdt(&self) -> Option<fdt::Fdt<'_>> {
		*self
	}

	fn fdt_addr(&self) -> Option<NonZero<usize>> {
		*self
	}
}
