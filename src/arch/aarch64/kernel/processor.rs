use core::arch::asm;
use core::{fmt, mem};

use aarch64_cpu::registers::*;
use hermit_sync::{Lazy, OnceCell, without_interrupts};

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
pub fn halt() {
	aarch64_cpu::asm::wfi();
}

/// Shutdown the system
#[allow(unused_variables)]
pub fn shutdown(error_code: i32) -> ! {
	info!("Shutting down system");

	cfg_select! {
		feature = "semihosting" => {
			semihosting::process::exit(error_code)
		}
		_ => {
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
	let Some(wt) = wakeup_time else {
		// disable timer
		CNTP_CVAL_EL0.set(0);
		CNTP_CTL_EL0.write(CNTP_CTL_EL0::ENABLE::CLEAR);
		return;
	};

	// wt is the absolute wakeup time in microseconds based on processor::get_timer_ticks.
	let freq: u64 = CPU_FREQUENCY.get().into(); // frequency in KHz
	let deadline = (wt / 1000) * freq;

	CNTP_CVAL_EL0.set(deadline);
	CNTP_CTL_EL0.write(CNTP_CTL_EL0::ENABLE::SET);
}

pub fn set_oneshot_timer(wakeup_time: Option<u64>) {
	without_interrupts(|| {
		__set_oneshot_timer(wakeup_time);
	});
}

const ARM_PARTS: &[(u64, &str)] = &[
	(0x810, "ARM810"),
	(0x920, "ARM920"),
	(0x922, "ARM922"),
	(0x926, "ARM926"),
	(0x940, "ARM940"),
	(0x946, "ARM946"),
	(0x966, "ARM966"),
	(0xa20, "ARM1020"),
	(0xa22, "ARM1022"),
	(0xa26, "ARM1026"),
	(0xb02, "ARM11-MPCore"),
	(0xb36, "ARM1136"),
	(0xb56, "ARM1156"),
	(0xb76, "ARM1176"),
	(0xc05, "Cortex-A5"),
	(0xc07, "Cortex-A7"),
	(0xc08, "Cortex-A8"),
	(0xc09, "Cortex-A9"),
	(0xc0d, "Cortex-A17"),
	(0xc0f, "Cortex-A15"),
	(0xc0e, "Cortex-A17"),
	(0xc14, "Cortex-R4"),
	(0xc15, "Cortex-R5"),
	(0xc17, "Cortex-R7"),
	(0xc18, "Cortex-R8"),
	(0xc20, "Cortex-M0"),
	(0xc21, "Cortex-M1"),
	(0xc23, "Cortex-M3"),
	(0xc24, "Cortex-M4"),
	(0xc27, "Cortex-M7"),
	(0xc60, "Cortex-M0+"),
	(0xd01, "Cortex-A32"),
	(0xd02, "Cortex-A34"),
	(0xd03, "Cortex-A53"),
	(0xd04, "Cortex-A35"),
	(0xd05, "Cortex-A55"),
	(0xd06, "Cortex-A65"),
	(0xd07, "Cortex-A57"),
	(0xd08, "Cortex-A72"),
	(0xd09, "Cortex-A73"),
	(0xd0a, "Cortex-A75"),
	(0xd0b, "Cortex-A76"),
	(0xd0c, "Neoverse-N1"),
	(0xd0d, "Cortex-A77"),
	(0xd0e, "Cortex-A76AE"),
	(0xd13, "Cortex-R52"),
	(0xd14, "Cortex-R82AE"),
	(0xd15, "Cortex-R82"),
	(0xd16, "Cortex-R52+"),
	(0xd20, "Cortex-M23"),
	(0xd21, "Cortex-M33"),
	(0xd24, "Cortex-M52"),
	(0xd22, "Cortex-M55"),
	(0xd23, "Cortex-M85"),
	(0xd40, "Neoverse-V1"),
	(0xd41, "Cortex-A78"),
	(0xd42, "Cortex-A78AE"),
	(0xd43, "Cortex-A65AE"),
	(0xd44, "Cortex-X1"),
	(0xd46, "Cortex-A510"),
	(0xd47, "Cortex-A710"),
	(0xd48, "Cortex-X2"),
	(0xd49, "Neoverse-N2"),
	(0xd4a, "Neoverse-E1"),
	(0xd4b, "Cortex-A78C"),
	(0xd4c, "Cortex-X1C"),
	(0xd4d, "Cortex-A715"),
	(0xd4e, "Cortex-X3"),
	(0xd4f, "Neoverse-V2"),
	(0xd80, "Cortex-A520"),
	(0xd81, "Cortex-A720"),
	(0xd82, "Cortex-X4"),
	(0xd83, "Neoverse-V3AE"),
	(0xd84, "Neoverse-V3"),
	(0xd85, "Cortex-X925"),
	(0xd87, "Cortex-A725"),
	(0xd88, "Cortex-A520AE"),
	(0xd89, "Cortex-A720AE"),
	(0xd8a, "C1-Nano"),
	(0xd8b, "C1-Pro"),
	(0xd8c, "C1-Ultra"),
	(0xd8e, "Neoverse-N3"),
	(0xd8f, "Cortex-A320"),
	(0xd90, "C1-Premium"),
];

const BRCM_PARTS: &[(u64, &str)] = &[
	(0x0f, "Brahma-B15"),
	(0x100, "Brahma-B53"),
	(0x516, "ThunderX2"),
];

const DEC_PARTS: &[(u64, &str)] = &[(0xa10, "SA110"), (0xa11, "SA1100")];

const CAVIUM_PARTS: &[(u64, &str)] = &[
	(0x0a0, "ThunderX"),
	(0x0a1, "ThunderX-88XX"),
	(0x0a2, "ThunderX-81XX"),
	(0x0a3, "ThunderX-83XX"),
	(0x0af, "ThunderX2-99xx"),
	(0x0b0, "OcteonTX2"),
	(0x0b1, "OcteonTX2-98XX"),
	(0x0b2, "OcteonTX2-96XX"),
	(0x0b3, "OcteonTX2-95XX"),
	(0x0b4, "OcteonTX2-95XXN"),
	(0x0b5, "OcteonTX2-95XXMM"),
	(0x0b6, "OcteonTX2-95XXO"),
	(0x0b8, "ThunderX3-T110"),
];

const APM_PARTS: &[(u64, &str)] = &[(0x000, "X-Gene")];

const QCOM_PARTS: &[(u64, &str)] = &[
	(0x001, "Oryon"),
	(0x00f, "Scorpion"),
	(0x02d, "Scorpion"),
	(0x04d, "Krait"),
	(0x06f, "Krait"),
	(0x201, "Kryo"),
	(0x205, "Kryo"),
	(0x211, "Kryo"),
	(0x800, "Falkor-V1/Kryo"),
	(0x801, "Kryo-V2"),
	(0x802, "Kryo-3XX-Gold"),
	(0x803, "Kryo-3XX-Silver"),
	(0x804, "Kryo-4XX-Gold"),
	(0x805, "Kryo-4XX-Silver"),
	(0xc00, "Falkor"),
	(0xc01, "Saphira"),
];

const SAMSUNG_PARTS: &[(u64, &str)] = &[
	(0x001, "exynos-m1"),
	(0x002, "exynos-m3"),
	(0x003, "exynos-m4"),
	(0x004, "exynos-m5"),
];

const NVIDIA_PARTS: &[(u64, &str)] = &[
	(0x000, "Denver"),
	(0x003, "Denver-2"),
	(0x004, "Carmel"),
	(0x010, "Olympus"),
];

const MARVELL_PARTS: &[(u64, &str)] = &[
	(0x131, "Feroceon-88FR131"),
	(0x581, "PJ4/PJ4b"),
	(0x584, "PJ4B-MP"),
];

const APPLE_PARTS: &[(u64, &str)] = &[
	(0x000, "Swift"),
	(0x001, "Cyclone"),
	(0x002, "Typhoon"),
	(0x003, "Typhoon/Capri"),
	(0x004, "Twister"),
	(0x005, "Twister/Elba/Malta"),
	(0x006, "Hurricane"),
	(0x007, "Hurricane/Myst"),
	(0x008, "Monsoon"),
	(0x009, "Mistral"),
	(0x00b, "Vortex"),
	(0x00c, "Tempest"),
	(0x00f, "Tempest-M9"),
	(0x010, "Vortex/Aruba"),
	(0x011, "Tempest/Aruba"),
	(0x012, "Lightning"),
	(0x013, "Thunder"),
	(0x020, "Icestorm-A14"),
	(0x021, "Firestorm-A14"),
	(0x022, "Icestorm-M1"),
	(0x023, "Firestorm-M1"),
	(0x024, "Icestorm-M1-Pro"),
	(0x025, "Firestorm-M1-Pro"),
	(0x026, "Thunder-M10"),
	(0x028, "Icestorm-M1-Max"),
	(0x029, "Firestorm-M1-Max"),
	(0x030, "Blizzard-A15"),
	(0x031, "Avalanche-A15"),
	(0x032, "Blizzard-M2"),
	(0x033, "Avalanche-M2"),
	(0x034, "Blizzard-M2-Pro"),
	(0x035, "Avalanche-M2-Pro"),
	(0x036, "Sawtooth-A16"),
	(0x037, "Everest-A16"),
	(0x038, "Blizzard-M2-Max"),
	(0x039, "Avalanche-M2-Max"),
];

const FARADAY_PARTS: &[(u64, &str)] = &[(0x526, "FA526"), (0x626, "FA626")];

const INTEL_PARTS: &[(u64, &str)] = &[
	(0x200, "i80200"),
	(0x210, "PXA250A"),
	(0x212, "PXA210A"),
	(0x242, "i80321-400"),
	(0x243, "i80321-600"),
	(0x290, "PXA250B/PXA26x"),
	(0x292, "PXA210B"),
	(0x2c2, "i80321-400-B0"),
	(0x2c3, "i80321-600-B0"),
	(0x2d0, "PXA250C/PXA255/PXA26x"),
	(0x2d2, "PXA210C"),
	(0x411, "PXA27x"),
	(0x41c, "IPX425-533"),
	(0x41d, "IPX425-400"),
	(0x41f, "IPX425-266"),
	(0x682, "PXA32x"),
	(0x683, "PXA930/PXA935"),
	(0x688, "PXA30x"),
	(0x689, "PXA31x"),
	(0xb11, "SA1110"),
	(0xc12, "IPX1200"),
];

const FUJITSU_PARTS: &[(u64, &str)] = &[(0x001, "A64FX"), (0x003, "MONAKA")];

const HISI_PARTS: &[(u64, &str)] = &[
	(0xd01, "TaiShan-v110"),
	(0xd02, "TaiShan-v120"),
	(0xd40, "Cortex-A76"),
	(0xd41, "Cortex-A77"),
];

const AMPERE_PARTS: &[(u64, &str)] = &[(0xac3, "Ampere-1"), (0xac4, "Ampere-1a")];

const FT_PARTS: &[(u64, &str)] = &[
	(0x303, "FTC310"),
	(0x660, "FTC660"),
	(0x661, "FTC661"),
	(0x662, "FTC662"),
	(0x663, "FTC663"),
	(0x664, "FTC664"),
	(0x862, "FTC862"),
];

const MS_PARTS: &[(u64, &str)] = &[(0xd49, "Azure-Cobalt-100")];

pub fn print_information() {
	// For implementer and part IDs, see util-linux source:
	// https://github.com/util-linux/util-linux/blob/da322604a45f8094d439ea64be7d44cfea1f500a/sys-utils/lscpu-arm.c

	// The implementer enum in aarch64-cpu is very incomplete
	let (implementer, parts): (&'static str, &[(u64, &str)]) =
		match MIDR_EL1.read(MIDR_EL1::Implementer) {
			0x41 => ("ARM", ARM_PARTS),
			0x42 => ("Broadcom", BRCM_PARTS),
			0x43 => ("Cavium", CAVIUM_PARTS),
			0x44 => ("DEC", DEC_PARTS),
			0x46 => ("FUJITSU", FUJITSU_PARTS),
			0x48 => ("HiSilicon", HISI_PARTS),
			0x49 => ("Infineon", &[]),           // no parts known
			0x4d => ("Motorola/Freescale", &[]), // no parts known
			0x4e => ("NVIDIA", NVIDIA_PARTS),
			0x50 => ("APM", APM_PARTS),
			0x51 => ("Qualcomm", QCOM_PARTS),
			0x53 => ("Samsung", SAMSUNG_PARTS),
			0x56 => ("Marvell", MARVELL_PARTS),
			0x61 => ("Apple", APPLE_PARTS),
			0x66 => ("Faraday", FARADAY_PARTS),
			0x69 => ("Intel", INTEL_PARTS),
			0x6d => ("Microsoft", MS_PARTS),
			0x70 => ("Phytium", FT_PARTS),
			0xc0 => ("Ampere", AMPERE_PARTS),
			_ => ("Unknown implementer", &[]),
		};

	let part_id = MIDR_EL1.read(MIDR_EL1::PartNum);
	let part = parts
		.iter()
		.find_map(|(id, name)| if *id == part_id { Some(*name) } else { None })
		.unwrap_or("Unknown part");

	let cpu_freq = &*CPU_FREQUENCY;

	infoheader!(" CPU INFORMATION ");
	infoentry!("Processor", "{implementer} {part}");
	infoentry!("Counter frequency", "{cpu_freq}");
	infofooter!();
}
