use core::num::NonZero;

use super::StartInfo;

#[cfg(not(any(target_arch = "aarch64", target_arch = "riscv64")))]
pub fn start_info() -> &'static impl StartInfo {
	#[expect(unreachable_code)]
	&panic!()
}

#[cfg(any(target_arch = "aarch64", target_arch = "riscv64"))]
pub fn start_info() -> &'static (impl StartInfo + super::FdtStartInfo) {
	#[expect(unreachable_code)]
	&panic!()
}

unsafe impl StartInfo for ! {
	fn bootargs(&self) -> Option<&str> {
		*self
	}

	fn rsdp_addr(&self) -> Option<NonZero<usize>> {
		*self
	}
}

#[cfg(any(target_arch = "aarch64", target_arch = "riscv64"))]
unsafe impl super::FdtStartInfo for ! {
	fn fdt(&self) -> Option<fdt::Fdt<'_>> {
		*self
	}

	fn fdt_addr(&self) -> Option<NonZero<usize>> {
		*self
	}
}
