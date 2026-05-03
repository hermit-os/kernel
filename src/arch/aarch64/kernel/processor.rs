use core::arch::asm;
use core::{fmt, mem};

use aarch64_cpu::registers::*;
use hermit_sync::{Lazy, OnceCell, without_interrupts};

use super::lscpu;
use crate::env;

/// Current FPU state. Saved at context switch when changed.
///
/// AArch64 mandates 32 NEON SIMD registers, which are named v0-v32.
///
/// See the Arm documentation for more information:
/// <https://developer.arm.com/documentation/102374/0103/Registers-in-AArch64---general-purpose-registers>
///
/// FPCR is the floating point control register and controls things like NaN
/// propagation, FPSR contains info like carry condition and over overflow
/// condition. These are callee-saved bits.
#[derive(Clone, Copy, Debug)]
pub struct FPUState {
	/// Advanced SIMD 128-bit vector registers.
	q: [u128; 32],
	/// FPCR register.
	fpcr: u64,
	/// FPSR register.
	fpsr: u64,
}

impl FPUState {
	pub fn new() -> Self {
		Self {
			q: [0; 32],
			fpcr: 0,
			fpsr: 0,
		}
	}

	pub fn restore(&self) {
		trace!("Restore FPUState at {self:p}");

		unsafe {
			asm!(
				".arch_extension fp",
				"ldp  q0,  q1, [{fpu_state}, {off_q} + 16 *  0]",
				"ldp  q2,  q3, [{fpu_state}, {off_q} + 16 *  2]",
				"ldp  q4,  q5, [{fpu_state}, {off_q} + 16 *  4]",
				"ldp  q6,  q7, [{fpu_state}, {off_q} + 16 *  6]",
				"ldp  q8,  q9, [{fpu_state}, {off_q} + 16 *  8]",
				"ldp q10, q11, [{fpu_state}, {off_q} + 16 * 10]",
				"ldp q12, q13, [{fpu_state}, {off_q} + 16 * 12]",
				"ldp q14, q15, [{fpu_state}, {off_q} + 16 * 14]",
				"ldp q16, q17, [{fpu_state}, {off_q} + 16 * 16]",
				"ldp q18, q19, [{fpu_state}, {off_q} + 16 * 18]",
				"ldp q20, q21, [{fpu_state}, {off_q} + 16 * 20]",
				"ldp q22, q23, [{fpu_state}, {off_q} + 16 * 22]",
				"ldp q24, q25, [{fpu_state}, {off_q} + 16 * 24]",
				"ldp q26, q27, [{fpu_state}, {off_q} + 16 * 26]",
				"ldp q28, q29, [{fpu_state}, {off_q} + 16 * 28]",
				"ldp q30, q31, [{fpu_state}, {off_q} + 16 * 30]",
				"ldr {intermediate}, [{fpu_state}, {off_fpcr}]",
				"msr fpcr, {intermediate}",
				"ldr {intermediate}, [{fpu_state}, {off_fpsr}]",
				"msr fpsr, {intermediate}",
				".arch_extension nofp",
				fpu_state = in(reg) self,
				off_q = const mem::offset_of!(FPUState, q),
				off_fpcr = const mem::offset_of!(FPUState, fpcr),
				off_fpsr = const mem::offset_of!(FPUState, fpsr),
				intermediate = out(reg) _,
			);
		}
	}

	pub fn save(&mut self) {
		trace!("Save FPUState at {self:p}");

		unsafe {
			asm!(
				".arch_extension fp",
				"stp  q0,  q1, [{fpu_state}, {off_q} + 16 *  0]",
				"stp  q2,  q3, [{fpu_state}, {off_q} + 16 *  2]",
				"stp  q4,  q5, [{fpu_state}, {off_q} + 16 *  4]",
				"stp  q6,  q7, [{fpu_state}, {off_q} + 16 *  6]",
				"stp  q8,  q9, [{fpu_state}, {off_q} + 16 *  8]",
				"stp q10, q11, [{fpu_state}, {off_q} + 16 * 10]",
				"stp q12, q13, [{fpu_state}, {off_q} + 16 * 12]",
				"stp q14, q15, [{fpu_state}, {off_q} + 16 * 14]",
				"stp q16, q17, [{fpu_state}, {off_q} + 16 * 16]",
				"stp q18, q19, [{fpu_state}, {off_q} + 16 * 18]",
				"stp q20, q21, [{fpu_state}, {off_q} + 16 * 20]",
				"stp q22, q23, [{fpu_state}, {off_q} + 16 * 22]",
				"stp q24, q25, [{fpu_state}, {off_q} + 16 * 24]",
				"stp q26, q27, [{fpu_state}, {off_q} + 16 * 26]",
				"stp q28, q29, [{fpu_state}, {off_q} + 16 * 28]",
				"stp q30, q31, [{fpu_state}, {off_q} + 16 * 30]",
				"mrs {intermediate}, fpcr",
				"str {intermediate}, [{fpu_state}, {off_fpcr}]",
				"mrs {intermediate}, fpsr",
				"str {intermediate}, [{fpu_state}, {off_fpsr}]",
				".arch_extension nofp",
				fpu_state = in(reg) self,
				off_q = const mem::offset_of!(FPUState, q),
				off_fpcr = const mem::offset_of!(FPUState, fpcr),
				off_fpsr = const mem::offset_of!(FPUState, fpsr),
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
// Value of CNTVCT_EL0 at boot time. We use the Virtual Timer (CNTV_*)
// rather than the Physical Timer (CNTP_*): the virtual timer is always
// available — including under hypervisors that hide CNTP_* from EL1
// (e.g. macOS HVF on Apple Silicon, where MSR/MRS to `CNTP_CVAL_EL0`
// faults as UNDEF) — and behaves identically on bare metal because
// CNTVOFF_EL2 defaults to zero when no hypervisor is in play.
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
		if khz == 0 {
			return Err(());
		}

		self.khz = khz;
		self.source = source;
		Ok(())
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
#[allow(dead_code)]
pub fn halt() {
	aarch64_cpu::asm::wfi();
}

/// Shutdown the system.
///
/// We try PSCI first, with semihosting as a fallback. Rationale:
///
/// * **PSCI `SYSTEM_OFF`** is implemented by QEMU's `virt` machine for
///   both `accel=tcg` and `accel=hvf`/`accel=kvm`, and by real-hardware
///   firmware. It is the most portable shutdown primitive on AArch64.
///
/// * **AArch64 semihosting** uses `HLT #0xf000`. On `accel=tcg`, QEMU
///   intercepts that instruction and exits with the supplied code.
///   Under `accel=hvf`/`accel=kvm` the HLT goes straight to the guest
///   as an UNDEF exception (`EC=0x0`) because hardware virtualisation
///   does not trap it — so semihosting is *not* a viable shutdown
///   primitive in a virtualised guest. We therefore use it only as a
///   fallback for the `_exit(error_code)` semantic when PSCI declines
///   to terminate (e.g. on platforms without a PSCI dispatcher).
#[allow(unused_variables)]
pub fn shutdown(error_code: i32) -> ! {
	info!("Shutting down system");

	unsafe {
		const PSCI_SYSTEM_OFF: u64 = 0x8400_0008;
		// hvc #0 with x0 = PSCI_SYSTEM_OFF: QEMU virt's PSCI dispatcher
		// terminates the VM. On real hardware this returns to firmware
		// which performs the shutdown.
		asm!("hvc #0", in("x0") PSCI_SYSTEM_OFF, options(nomem, nostack));
	}

	// PSCI did not terminate (no dispatcher, or call returned). Fall back
	// to semihosting if the feature is compiled in — useful under TCG
	// where it correctly propagates `error_code` to the host shell.
	if cfg!(feature = "semihosting") {
		semihosting::process::exit(error_code)
	} else {
		// Last resort: park the CPU forever.
		loop {
			aarch64_cpu::asm::wfe();
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
	CNTVCT_EL0.get() - BOOT_COUNTER.get().unwrap()
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
	#[cfg(feature = "uhyve")]
	if env::is_uhyve() {
		return;
	}

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
	BOOT_COUNTER.set(CNTVCT_EL0.get()).unwrap();
	Lazy::force(&CPU_FREQUENCY);
}

#[inline]
fn __set_oneshot_timer(wakeup_time: Option<u64>) {
	let Some(wt) = wakeup_time else {
		// disable timer
		CNTV_CVAL_EL0.set(0);
		CNTV_CTL_EL0.write(CNTV_CTL_EL0::ENABLE::CLEAR);
		return;
	};

	// wt is the absolute wakeup time in microseconds based on processor::get_timer_ticks.
	let freq: u64 = CPU_FREQUENCY.get().into(); // frequency in KHz
	let deadline = (wt / 1000) * freq;

	CNTV_CVAL_EL0.set(deadline);
	CNTV_CTL_EL0.write(CNTV_CTL_EL0::ENABLE::SET);
}

pub fn set_oneshot_timer(wakeup_time: Option<u64>) {
	without_interrupts(|| {
		__set_oneshot_timer(wakeup_time);
	});
}

pub fn print_information() {
	// Implementer is 8 bits wide, so this cannot fail
	let implementer_raw = u8::try_from(MIDR_EL1.read(MIDR_EL1::Implementer)).unwrap();
	// PartNum is 12 bits wide, so this cannot fail
	let part_raw = u16::try_from(MIDR_EL1.read(MIDR_EL1::PartNum)).unwrap();

	let (implementer, part) = match lscpu::pretty_implementer_and_part(implementer_raw, part_raw) {
		Some((implementer, Some(part))) => (implementer, part),
		Some((implementer, None)) => (implementer, "Unknown part"),
		None => ("Unknown implementer", "Unknown part"),
	};

	let cpu_freq = &*CPU_FREQUENCY;

	infoheader!(" CPU INFORMATION ");
	infoentry!("Processor", "{implementer} {part}");
	infoentry!("Counter frequency", "{cpu_freq}");
	infofooter!();
}
