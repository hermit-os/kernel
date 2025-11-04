use core::arch::asm;
use core::fmt;
use core::mem::offset_of;

use aarch64_cpu::registers::*;
use hermit_sync::{Lazy, OnceCell, without_interrupts};

use crate::env;

/// Current FPU state. Saved at context switch when changed.
///
/// AArch64 mandates 32 NEON SIMD registers, which are named v0-v32.
/// Only the lower 64 bits of v8-v15 must be saved (the d parts).
///
/// See the ABI documentation for aarch64 on this topic:
/// <https://github.com/ARM-software/abi-aa/blob/main/aapcs64/aapcs64.rst#612simd-and-floating-point-registers>
///
/// FPCR is the floating point control register and controls things like NaN
/// propagation, FPSR contains info like carry condition and over overflow
/// condition. These are callee-saved bits.
#[derive(Clone, Copy, Debug)]
pub struct FPUState {
	/// d8 register
	d8: u64,
	/// d9 register
	d9: u64,
	/// d10 register
	d10: u64,
	/// d11 register
	d11: u64,
	/// d12 register
	d12: u64,
	/// d13 register
	d13: u64,
	/// d14 register
	d14: u64,
	/// d15 register
	d15: u64,
	/// fpcr register.
	fpcr: u64,
	/// fpsr register.
	fpsr: u64,
}

impl FPUState {
	pub fn new() -> Self {
		Self {
			d8: 0,
			d9: 0,
			d10: 0,
			d11: 0,
			d12: 0,
			d13: 0,
			d14: 0,
			d15: 0,
			fpcr: 0,
			fpsr: 0,
		}
	}

	pub fn restore(&self) {
		trace!("Restore FPUState at {self:p}");

		unsafe {
			asm!(
				".arch_extension fp",
				"ldr d8, [{fpu_state}, {off_d8}]",
				"ldr d9, [{fpu_state}, {off_d9}]",
				"ldr d10, [{fpu_state}, {off_d10}]",
				"ldr d11, [{fpu_state}, {off_d11}]",
				"ldr d12, [{fpu_state}, {off_d12}]",
				"ldr d13, [{fpu_state}, {off_d13}]",
				"ldr d14, [{fpu_state}, {off_d14}]",
				"ldr d15, [{fpu_state}, {off_d15}]",
				"ldr {intermediate}, [{fpu_state}, {off_fpcr}]",
				"msr fpcr, {intermediate}",
				"ldr {intermediate}, [{fpu_state}, {off_fpsr}]",
				"msr fpsr, {intermediate}",
				".arch_extension nofp",
				fpu_state = in(reg) self,
				off_d8 = const offset_of!(FPUState, d8),
				off_d9 = const offset_of!(FPUState, d9),
				off_d10 = const offset_of!(FPUState, d10),
				off_d11 = const offset_of!(FPUState, d11),
				off_d12 = const offset_of!(FPUState, d12),
				off_d13 = const offset_of!(FPUState, d13),
				off_d14 = const offset_of!(FPUState, d14),
				off_d15 = const offset_of!(FPUState, d15),
				off_fpcr = const offset_of!(FPUState, fpcr),
				off_fpsr = const offset_of!(FPUState, fpsr),
				intermediate = out(reg) _,
			);
		}
	}

	pub fn save(&mut self) {
		trace!("Save FPUState at {self:p}");

		unsafe {
			asm!(
				".arch_extension fp",
				"str d8, [{fpu_state}, {off_d8}]",
				"str d9, [{fpu_state}, {off_d9}]",
				"str d10, [{fpu_state}, {off_d10}]",
				"str d11, [{fpu_state}, {off_d11}]",
				"str d12, [{fpu_state}, {off_d12}]",
				"str d13, [{fpu_state}, {off_d13}]",
				"str d14, [{fpu_state}, {off_d14}]",
				"str d15, [{fpu_state}, {off_d15}]",
				"mrs {intermediate}, fpcr",
				"str {intermediate}, [{fpu_state}, {off_fpcr}]",
				"mrs {intermediate}, fpsr",
				"str {intermediate}, [{fpu_state}, {off_fpsr}]",
				".arch_extension nofp",
				fpu_state = in(reg) self,
				off_d8 = const offset_of!(FPUState, d8),
				off_d9 = const offset_of!(FPUState, d9),
				off_d10 = const offset_of!(FPUState, d10),
				off_d11 = const offset_of!(FPUState, d11),
				off_d12 = const offset_of!(FPUState, d12),
				off_d13 = const offset_of!(FPUState, d13),
				off_d14 = const offset_of!(FPUState, d14),
				off_d15 = const offset_of!(FPUState, d15),
				off_fpcr = const offset_of!(FPUState, fpcr),
				off_fpsr = const offset_of!(FPUState, fpsr),
				intermediate = out(reg) _,
			);
		}
	}
}

// System counter frequency in KHz
static CPU_FREQUENCY: Lazy<CpuFrequency> = Lazy::new(|| {
	let mut cpu_frequency = CpuFrequency::new();
	unsafe {
		cpu_frequency.detect();
	}
	cpu_frequency
});
// Value of CNTPCT_EL0 at boot time
static BOOT_COUNTER: OnceCell<u64> = OnceCell::new();

enum CpuFrequencySources {
	Invalid,
	CommandLine,
	Register,
}

impl fmt::Display for CpuFrequencySources {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match &self {
			CpuFrequencySources::CommandLine => write!(f, "Command Line"),
			CpuFrequencySources::Register => write!(f, "CNTFRQ_EL0"),
			CpuFrequencySources::Invalid => {
				panic!("Attempted to print an invalid CPU Frequency Source")
			}
		}
	}
}

struct CpuFrequency {
	khz: u32,
	source: CpuFrequencySources,
}

impl CpuFrequency {
	const fn new() -> Self {
		CpuFrequency {
			khz: 0,
			source: CpuFrequencySources::Invalid,
		}
	}

	fn set_detected_cpu_frequency(
		&mut self,
		khz: u32,
		source: CpuFrequencySources,
	) -> Result<(), ()> {
		//The clock frequency must never be set to zero, otherwise a division by zero will
		//occur during runtime
		if khz > 0 {
			self.khz = khz;
			self.source = source;
			Ok(())
		} else {
			Err(())
		}
	}

	unsafe fn detect_from_cmdline(&mut self) -> Result<(), ()> {
		let mhz = env::freq().ok_or(())?;
		self.set_detected_cpu_frequency(u32::from(mhz) * 1000, CpuFrequencySources::CommandLine)
	}

	unsafe fn detect_from_register(&mut self) -> Result<(), ()> {
		let khz = (CNTFRQ_EL0.get() & 0xffff_ffff) / 1000;
		self.set_detected_cpu_frequency(khz.try_into().unwrap(), CpuFrequencySources::Register)
	}

	unsafe fn detect(&mut self) {
		unsafe {
			self.detect_from_register()
				.or_else(|_e| self.detect_from_cmdline())
				.unwrap();
		}
	}

	fn get(&self) -> u32 {
		self.khz
	}
}

impl fmt::Display for CpuFrequency {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "{} KHz (from {})", self.khz, self.source)
	}
}

pub fn seed_entropy() -> Option<[u8; 32]> {
	None
}

/// The halt function stops the processor until the next interrupt arrives
pub fn halt() {
	aarch64_cpu::asm::wfi();
}

/// Shutdown the system
#[allow(unused_variables)]
pub fn shutdown(error_code: i32) -> ! {
	info!("Shutting down system");

	cfg_if::cfg_if! {
		if #[cfg(feature = "semihosting")] {
			semihosting::process::exit(error_code)
		} else {
			unsafe {
				const PSCI_SYSTEM_OFF: u64 = 0x8400_0008;
				// call hypervisor to shut down the system
				asm!("hvc #0", in("x0") PSCI_SYSTEM_OFF, options(nomem, nostack));

				// we should never reach this point
				loop {
					aarch64_cpu::asm::wfe();
				}
			}
		}
	}
}

#[inline]
pub fn get_timer_ticks() -> u64 {
	// We simulate a timer with a 1 microsecond resolution by taking the CPU timestamp
	// and dividing it by the CPU frequency (in KHz).

	let freq: u64 = CPU_FREQUENCY.get().into(); // frequency in KHz
	1000 * get_timestamp() / freq
}

/// Returns the timer frequency in MHz
#[inline]
pub fn get_frequency() -> u16 {
	(CPU_FREQUENCY.get() / 1_000).try_into().unwrap()
}

#[inline]
pub fn get_timestamp() -> u64 {
	CNTPCT_EL0.get() - BOOT_COUNTER.get().unwrap()
}

#[inline]
#[allow(dead_code)]
pub fn supports_1gib_pages() -> bool {
	false
}

#[inline]
pub fn supports_2mib_pages() -> bool {
	false
}

pub fn configure() {
	// TODO: PMCCNTR_EL0 is the best replacement for RDTSC on AArch64.
	// However, this test code showed that it's apparently not supported under uhyve yet.
	// Finish the boot loader for QEMU first and then run this code under QEMU, where it should be supported.
	// If that's the case, find out what's wrong with uhyve.
	unsafe {
		// TODO: Setting PMUSERENR_EL0 is probably not required, but find out about that
		// when reading PMCCNTR_EL0 works at all.
		let pmuserenr_el0: u64 = (1 << 0) | (1 << 2) | (1 << 3);
		asm!(
			"msr pmuserenr_el0, {}",
			in(reg) pmuserenr_el0,
			options(nostack, nomem),
		);

		// TODO: Setting PMCNTENSET_EL0 is probably not required, but find out about that
		// when reading PMCCNTR_EL0 works at all.
		let pmcntenset_el0: u64 = 1 << 31;
		asm!(
			"msr pmcntenset_el0, {}",
			in(reg) pmcntenset_el0,
			options(nostack, nomem),
		);

		// Enable PMCCNTR_EL0 using PMCR_EL0.
		let mut pmcr_el0: u64;
		asm!(
			"mrs {}, pmcr_el0",
			out(reg) pmcr_el0,
			options(nostack, nomem),
		);
		debug!("PMCR_EL0 (has RES1 bits and therefore mustn't be zero): {pmcr_el0:#X}");
		pmcr_el0 |= (1 << 0) | (1 << 2) | (1 << 6);
		asm!(
			"msr pmcr_el0, {}",
			in(reg) pmcr_el0,
			options(nostack, nomem),
		);
	}
}

pub fn detect_frequency() {
	BOOT_COUNTER.set(CNTPCT_EL0.get()).unwrap();
	Lazy::force(&CPU_FREQUENCY);
}

#[inline]
fn __set_oneshot_timer(wakeup_time: Option<u64>) {
	if let Some(wt) = wakeup_time {
		// wt is the absolute wakeup time in microseconds based on processor::get_timer_ticks.
		let freq: u64 = CPU_FREQUENCY.get().into(); // frequency in KHz
		let deadline = (wt / 1000) * freq;

		CNTP_CVAL_EL0.set(deadline);
		CNTP_CTL_EL0.write(CNTP_CTL_EL0::ENABLE::SET);
	} else {
		// disable timer
		CNTP_CVAL_EL0.set(0);
		CNTP_CTL_EL0.write(CNTP_CTL_EL0::ENABLE::CLEAR);
	}
}

pub fn set_oneshot_timer(wakeup_time: Option<u64>) {
	without_interrupts(|| {
		__set_oneshot_timer(wakeup_time);
	});
}

pub fn print_information() {
	let fdt = env::fdt().unwrap();
	let cpu0 = fdt.cpus().next().unwrap();
	let cpu0_compatible = cpu0.property("compatible").unwrap().as_str().unwrap();

	infoheader!(" CPU INFORMATION ");
	infoentry!("Processor compatibility", cpu0_compatible);
	infoentry!("Counter frequency", *CPU_FREQUENCY);
	infofooter!();
}
