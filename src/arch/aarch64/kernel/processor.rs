use core::arch::asm;
use core::sync::atomic::spin_loop_hint;

extern "C" {
	static mut cpu_freq: u32;
}

pub struct FPUState {
	// TODO
}

impl FPUState {
	pub fn new() -> Self {
		Self {}
	}

	pub fn restore(&self) {
		// TODO
	}

	pub fn save(&self) {
		// TODO
	}
}

pub fn generate_random_number32() -> Option<u32> {
	None
}

pub fn generate_random_number64() -> Option<u64> {
	None
}

pub fn run_on_hypervisor() -> bool {
	true
}

/// Search the most significant bit
#[inline(always)]
pub fn msb(value: u64) -> Option<u64> {
	if value > 0 {
		let ret: u64;

		unsafe {
			asm!(
				"clz {0}, {1}",
				"sub {0}, {2}, {0}",
				out(reg) ret,
				in(reg) value,
				const 64 - 1,
				options(nostack, nomem),
			);
		}
		Some(ret)
	} else {
		None
	}
}

/// The halt function stops the processor until the next interrupt arrives
pub fn halt() {
	unsafe {
		asm!("wfi", options(nostack, nomem),);
	}
}

/// Shutdown the system
pub fn shutdown() -> ! {
	info!("Shutting down system");

	loop {
		halt();
	}
}

pub fn get_timer_ticks() -> u64 {
	// We simulate a timer with a 1 microsecond resolution by taking the CPU timestamp
	// and dividing it by the CPU frequency in MHz.
	0
}

pub fn get_frequency() -> u16 {
	0
}

#[inline]
pub fn get_timestamp() -> u64 {
	0
}

#[inline]
pub fn supports_1gib_pages() -> bool {
	false
}

#[inline]
pub fn supports_2mib_pages() -> bool {
	false
}

/// Delay execution by the given number of microseconds using busy-waiting.
#[inline]
pub fn udelay(usecs: u64) {
	let end = get_timestamp() + get_frequency() as u64 * usecs;
	while get_timestamp() < end {
		spin_loop_hint();
	}
}

pub fn configure() {
	// TODO: PMCCNTR_EL0 is the best replacement for RDTSC on AArch64.
	// However, this test code showed that it's apparently not supported under uhyve yet.
	// Finish the boot loader for QEMU first and then run this code under QEMU, where it should be supported.
	// If that's the case, find out what's wrong with uhyve.
	unsafe {
		// TODO: Setting PMUSERENR_EL0 is probably not required, but find out about that
		// when reading PMCCNTR_EL0 works at all.
		let pmuserenr_el0: u32 = 1 << 0 | 1 << 2 | 1 << 3;
		asm!(
			"msr pmuserenr_el0, {}",
			in(reg) pmuserenr_el0,
			options(nostack, nomem),
		);
		debug!("pmuserenr_el0");

		// TODO: Setting PMCNTENSET_EL0 is probably not required, but find out about that
		// when reading PMCCNTR_EL0 works at all.
		let pmcntenset_el0: u32 = 1 << 31;
		asm!(
			"msr pmcntenset_el0, {}",
			in(reg) pmcntenset_el0,
			options(nostack, nomem),
		);
		debug!("pmcntenset_el0");

		// Enable PMCCNTR_EL0 using PMCR_EL0.
		let mut pmcr_el0: u32 = 0;
		asm!(
			"mrs {}, pmcr_el0",
			out(reg) pmcr_el0,
			options(nostack, nomem),
		);
		debug!(
			"PMCR_EL0 (has RES1 bits and therefore musn't be zero): {:#X}",
			pmcr_el0
		);
		pmcr_el0 |= 1 << 0 | 1 << 2 | 1 << 6;
		asm!(
			"msr pmcr_el0, {}",
			in(reg) pmcr_el0,
			options(nostack, nomem),
		);
	}
}
