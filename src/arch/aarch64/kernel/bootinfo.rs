use core::{fmt, ptr};

#[repr(C)]
pub struct BootInfo {
	pub magic_number: u32,
	pub version: u32,
	pub base: u64,
	pub limit: u64,
	pub image_size: u64,
	pub tls_start: u64,
	pub tls_filesz: u64,
	pub tls_memsz: u64,
	pub current_stack_address: u64,
	pub current_percore_address: u64,
	pub host_logical_addr: u64,
	pub boot_gtod: u64,
	pub cmdline: u64,
	pub cmdsize: u64,
	pub cpu_freq: u32,
	pub boot_processor: u32,
	pub cpu_online: u32,
	pub possible_cpus: u32,
	pub current_boot_id: u32,
	pub uartport: u32,
	pub single_kernel: u8,
	pub uhyve: u8,
	pub hcip: [u8; 4],
	pub hcgateway: [u8; 4],
	pub hcmask: [u8; 4],
	pub tls_align: u64,
}

impl BootInfo {
	const LAYOUT: Self = Self {
		magic_number: 0,
		version: 0,
		base: 0,
		limit: 0,
		tls_start: 0,
		tls_filesz: 0,
		tls_memsz: 0,
		image_size: 0,
		current_stack_address: 0,
		current_percore_address: 0,
		host_logical_addr: 0,
		boot_gtod: 0,
		cmdline: 0,
		cmdsize: 0,
		cpu_freq: 0,
		boot_processor: 0,
		cpu_online: 0,
		possible_cpus: 0,
		current_boot_id: 0,
		uartport: 0,
		single_kernel: 0,
		uhyve: 0,
		hcip: [0; 4],
		hcgateway: [0; 4],
		hcmask: [0; 4],
		tls_align: 0,
	};

	pub const fn current_stack_address_offset() -> isize {
		let layout = Self::LAYOUT;
		let start = ptr::addr_of!(layout);
		let stack = ptr::addr_of!(layout.current_stack_address);
		unsafe { stack.cast::<u8>().offset_from(start.cast()) }
	}
}

impl fmt::Debug for BootInfo {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		writeln!(f, "magic_number {:#x}", self.magic_number)?;
		writeln!(f, "version {:#x}", self.version)?;
		writeln!(f, "base {:#x}", self.base)?;
		writeln!(f, "limit {:#x}", self.limit)?;
		writeln!(f, "tls_start {:#x}", self.tls_start)?;
		writeln!(f, "tls_filesz {:#x}", self.tls_filesz)?;
		writeln!(f, "tls_memsz {:#x}", self.tls_memsz)?;
		writeln!(f, "tls_align {:#x}", self.tls_align)?;
		writeln!(f, "image_size {:#x}", self.image_size)?;
		writeln!(f, "current_stack_address {:#x}", self.current_stack_address)?;
		writeln!(
			f,
			"current_percore_address {:#x}",
			self.current_percore_address
		)?;
		writeln!(f, "host_logical_addr {:#x}", self.host_logical_addr)?;
		writeln!(f, "boot_gtod {:#x}", self.boot_gtod)?;
		writeln!(f, "cmdline {:#x}", self.cmdline)?;
		writeln!(f, "cmdsize {:#x}", self.cmdsize)?;
		writeln!(f, "cpu_freq {}", self.cpu_freq)?;
		writeln!(f, "boot_processor {}", self.boot_processor)?;
		writeln!(f, "cpu_online {}", self.cpu_online)?;
		writeln!(f, "possible_cpus {}", self.possible_cpus)?;
		writeln!(f, "current_boot_id {}", self.current_boot_id)?;
		writeln!(f, "uartport {:#x}", self.uartport)?;
		writeln!(f, "single_kernel {}", self.single_kernel)?;
		writeln!(f, "uhyve {}", self.uhyve)
	}
}
