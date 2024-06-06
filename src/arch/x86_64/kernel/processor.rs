#![allow(dead_code)]

use core::arch::asm;
use core::arch::x86_64::{
	__rdtscp, _fxrstor, _fxsave, _mm_lfence, _rdseed64_step, _rdtsc, _xrstor, _xsave,
};
use core::hint::spin_loop;
use core::num::NonZeroU32;
use core::sync::atomic::{AtomicU64, Ordering};
use core::{fmt, ptr};

use hermit_entry::boot_info::PlatformInfo;
use hermit_sync::Lazy;
use x86::bits64::segmentation;
use x86::controlregs::*;
use x86::cpuid::*;
use x86::msr::*;
use x86_64::instructions::interrupts::int3;
use x86_64::instructions::port::Port;
use x86_64::instructions::tables::lidt;
use x86_64::structures::DescriptorTablePointer;
use x86_64::VirtAddr;

#[cfg(feature = "acpi")]
use crate::arch::x86_64::kernel::acpi;
use crate::arch::x86_64::kernel::{boot_info, interrupts, pic, pit};
use crate::env;

const IA32_MISC_ENABLE_ENHANCED_SPEEDSTEP: u64 = 1 << 16;
const IA32_MISC_ENABLE_SPEEDSTEP_LOCK: u64 = 1 << 20;
const IA32_MISC_ENABLE_TURBO_DISABLE: u64 = 1 << 38;

// MSR EFER bits
const EFER_SCE: u64 = 1 << 0;
const EFER_LME: u64 = 1 << 8;
const EFER_LMA: u64 = 1 << 10;
const EFER_NXE: u64 = 1 << 11;
const EFER_SVME: u64 = 1 << 12;
const EFER_LMSLE: u64 = 1 << 13;
const EFER_FFXSR: u64 = 1 << 14;
const EFER_TCE: u64 = 1 << 15;

// See Intel SDM - Volume 1 - Section 7.3.17.1
const RDRAND_RETRY_LIMIT: usize = 10;

static MEASUREMENT_TIMER_TICKS: AtomicU64 = AtomicU64::new(0);

#[derive(Debug)]
struct Features {
	physical_address_bits: u8,
	linear_address_bits: u8,
	supports_1gib_pages: bool,
	supports_avx: bool,
	supports_rdseed: bool,
	supports_tsc_deadline: bool,
	supports_x2apic: bool,
	supports_xsave: bool,
	supports_mwait: bool,
	supports_clflush: bool,
	run_on_hypervisor: bool,
	supports_fsgs: bool,
	supports_rdtscp: bool,
	cpu_speedstep: CpuSpeedStep,
}

static FEATURES: Lazy<Features> = Lazy::new(|| {
	// Detect CPU features
	let cpuid = CpuId::new();
	let feature_info = cpuid
		.get_feature_info()
		.expect("CPUID Feature Info not available!");
	let extended_feature_info = cpuid
		.get_extended_feature_info()
		.expect("CPUID Extended Feature Info not available!");
	let processor_capacity_info = cpuid
		.get_processor_capacity_feature_info()
		.expect("Processor Capacity Parameters and Extended Feature Identification not available!");
	let extend_processor_identifiers = cpuid
		.get_extended_processor_and_feature_identifiers()
		.expect("Extended Processor and Processor Feature Identifiers not available");
	Features {
		physical_address_bits: processor_capacity_info.physical_address_bits(),
		linear_address_bits: processor_capacity_info.linear_address_bits(),
		supports_1gib_pages: extend_processor_identifiers.has_1gib_pages(),
		supports_avx: feature_info.has_avx(),
		supports_rdseed: extended_feature_info.has_rdseed(),
		supports_tsc_deadline: feature_info.has_tsc_deadline(),
		supports_x2apic: feature_info.has_x2apic(),
		supports_xsave: feature_info.has_xsave(),
		supports_mwait: feature_info.has_monitor_mwait(),
		supports_clflush: feature_info.has_clflush(),
		run_on_hypervisor: feature_info.has_hypervisor(),
		supports_fsgs: extended_feature_info.has_fsgsbase(),
		supports_rdtscp: extend_processor_identifiers.has_rdtscp(),
		cpu_speedstep: {
			let mut cpu_speedstep = CpuSpeedStep::new();
			cpu_speedstep.detect_features(&cpuid);
			cpu_speedstep
		},
	}
});

static CPU_FREQUENCY: Lazy<CpuFrequency> = Lazy::new(|| {
	let mut cpu_frequency = CpuFrequency::new();
	unsafe {
		cpu_frequency.detect();
	}
	cpu_frequency
});

#[repr(C, align(16))]
pub struct XSaveLegacyRegion {
	pub fpu_control_word: u16,
	pub fpu_status_word: u16,
	pub fpu_tag_word: u16,
	pub fpu_opcode: u16,
	pub fpu_instruction_pointer: u32,
	pub fpu_instruction_pointer_high_or_cs: u32,
	pub fpu_data_pointer: u32,
	pub fpu_data_pointer_high_or_ds: u32,
	pub mxcsr: u32,
	pub mxcsr_mask: u32,
	pub st_space: [u8; 8 * 16],
	pub xmm_space: [u8; 16 * 16],
	pub padding: [u8; 96],
}

#[repr(C)]
pub struct XSaveHeader {
	pub xstate_bv: u64,
	pub xcomp_bv: u64,
	pub reserved: [u64; 6],
}

#[repr(C)]
pub struct XSaveAVXState {
	pub ymmh_space: [u8; 16 * 16],
}

/// XSave Area for AMD Lightweight Profiling.
/// Refer to AMD Lightweight Profiling Specification (Publication No. 43724), Figure 7-1.
#[repr(C)]
pub struct XSaveLWPState {
	pub lwpcb_address: u64,
	pub flags: u32,
	pub buffer_head_offset: u32,
	pub buffer_base: u64,
	pub buffer_size: u32,
	pub filters: u32,
	pub saved_event_record: [u64; 4],
	pub event_counter: [u32; 16],
}

#[repr(C)]
pub struct XSaveBndregs {
	pub bound_registers: [u8; 4 * 16],
}

#[repr(C)]
pub struct XSaveBndcsr {
	pub bndcfgu_register: u64,
	pub bndstatus_register: u64,
}

#[repr(C, align(64))]
pub struct FPUState {
	pub legacy_region: XSaveLegacyRegion,
	pub header: XSaveHeader,
	pub avx_state: XSaveAVXState,
	pub lwp_state: XSaveLWPState,
	pub bndregs: XSaveBndregs,
	pub bndcsr: XSaveBndcsr,
}

impl FPUState {
	pub const fn new() -> Self {
		Self {
			// Set FPU-related values to their default values after initialization.
			// Refer to Intel Vol. 3A, Table 9-1. IA-32 and Intel 64 Processor States Following Power-up, Reset, or INIT
			legacy_region: XSaveLegacyRegion {
				fpu_control_word: 0x37F,
				fpu_status_word: 0,
				fpu_tag_word: 0xFFFF,
				fpu_opcode: 0,
				fpu_instruction_pointer: 0,
				fpu_instruction_pointer_high_or_cs: 0,
				fpu_data_pointer: 0,
				fpu_data_pointer_high_or_ds: 0,
				mxcsr: 0x1F80,
				mxcsr_mask: 0,
				st_space: [0; 8 * 16],
				xmm_space: [0; 16 * 16],
				padding: [0; 96],
			},

			header: XSaveHeader {
				xstate_bv: 0,
				xcomp_bv: 0,
				reserved: [0; 6],
			},
			avx_state: XSaveAVXState {
				ymmh_space: [0; 16 * 16],
			},
			lwp_state: XSaveLWPState {
				lwpcb_address: 0,
				flags: 0,
				buffer_head_offset: 0,
				buffer_base: 0,
				buffer_size: 0,
				filters: 0,
				saved_event_record: [0; 4],
				event_counter: [0; 16],
			},
			bndregs: XSaveBndregs {
				bound_registers: [0; 4 * 16],
			},
			bndcsr: XSaveBndcsr {
				bndcfgu_register: 0,
				bndstatus_register: 0,
			},
		}
	}

	pub fn restore(&self) {
		if supports_xsave() {
			unsafe {
				_xrstor(ptr::from_ref(self) as _, u64::MAX);
			}
		} else {
			self.restore_common();
		}
	}

	pub fn save(&mut self) {
		if supports_xsave() {
			unsafe {
				_xsave(ptr::from_mut(self) as _, u64::MAX);
			}
		} else {
			self.save_common();
		}
	}

	pub fn restore_common(&self) {
		unsafe {
			_fxrstor(ptr::from_ref(self) as _);
		}
	}

	pub fn save_common(&mut self) {
		unsafe {
			_fxsave(ptr::from_mut(self) as _);
			asm!("fnclex", options(nomem, nostack));
		}
	}
}

enum CpuFrequencySources {
	Invalid,
	CommandLine,
	CpuIdBrandString,
	Measurement,
	Hypervisor,
	CpuId,
	CpuIdTscInfo,
	HypervisorTscInfo,
	Visionary,
}

impl fmt::Display for CpuFrequencySources {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match &self {
			CpuFrequencySources::CommandLine => write!(f, "Command Line"),
			CpuFrequencySources::CpuIdBrandString => write!(f, "CpuId Brand String"),
			CpuFrequencySources::Measurement => write!(f, "Measurement"),
			CpuFrequencySources::Hypervisor => write!(f, "Hypervisor"),
			CpuFrequencySources::CpuId => write!(f, "CpuId"),
			CpuFrequencySources::CpuIdTscInfo => write!(f, "CpuId Tsc Info"),
			CpuFrequencySources::HypervisorTscInfo => write!(f, "Tsc Info from Hypervisor"),
			CpuFrequencySources::Visionary => write!(f, "Visionary"),
			_ => panic!("Attempted to print an invalid CPU Frequency Source"),
		}
	}
}

struct CpuFrequency {
	mhz: u16,
	source: CpuFrequencySources,
}

impl CpuFrequency {
	const fn new() -> Self {
		CpuFrequency {
			mhz: 0,
			source: CpuFrequencySources::Invalid,
		}
	}

	fn set_detected_cpu_frequency(
		&mut self,
		mhz: u16,
		source: CpuFrequencySources,
	) -> Result<(), ()> {
		//The clock frequency must never be set to zero, otherwise a division by zero will
		//occur during runtime
		if mhz > 0 {
			self.mhz = mhz;
			self.source = source;
			Ok(())
		} else {
			Err(())
		}
	}

	unsafe fn detect_from_cmdline(&mut self) -> Result<(), ()> {
		let mhz = env::freq().ok_or(())?;
		self.set_detected_cpu_frequency(mhz, CpuFrequencySources::CommandLine)
	}

	unsafe fn detect_from_cpuid(&mut self, cpuid: &CpuId) -> Result<(), ()> {
		let processor_frequency_info = cpuid.get_processor_frequency_info();

		match processor_frequency_info {
			Some(freq_info) => {
				let mhz = freq_info.processor_base_frequency();
				self.set_detected_cpu_frequency(mhz, CpuFrequencySources::CpuId)
			}
			None => Err(()),
		}
	}

	unsafe fn detect_from_cpuid_tsc_info(&mut self, cpuid: &CpuId) -> Result<(), ()> {
		let tsc_info = cpuid.get_tsc_info().ok_or(())?;
		let freq = tsc_info.tsc_frequency().ok_or(())?;
		let mhz = (freq / 1000000u64) as u16;
		self.set_detected_cpu_frequency(mhz, CpuFrequencySources::CpuIdTscInfo)
	}

	unsafe fn detect_from_cpuid_hypervisor_info(&mut self, cpuid: &CpuId) -> Result<(), ()> {
		const KHZ_TO_HZ: u64 = 1000;
		const MHZ_TO_HZ: u64 = 1000000;
		let hypervisor_info = cpuid.get_hypervisor_info().ok_or(())?;
		let freq = hypervisor_info.tsc_frequency().ok_or(())? as u64 * KHZ_TO_HZ;
		let mhz: u16 = (freq / MHZ_TO_HZ).try_into().unwrap();
		self.set_detected_cpu_frequency(mhz, CpuFrequencySources::HypervisorTscInfo)
	}

	unsafe fn detect_from_cpuid_brand_string(&mut self, cpuid: &CpuId) -> Result<(), ()> {
		if let Some(processor_brand) = cpuid.get_processor_brand_string() {
			let brand_string = processor_brand.as_str();
			let ghz_find = brand_string.find("GHz");

			if let Some(ghz_find) = ghz_find {
				let index = ghz_find - 4;
				let thousand_char = brand_string.chars().nth(index).unwrap();
				let decimal_char = brand_string.chars().nth(index + 1).unwrap();
				let hundred_char = brand_string.chars().nth(index + 2).unwrap();
				let ten_char = brand_string.chars().nth(index + 3).unwrap();

				if let (Some(thousand), '.', Some(hundred), Some(ten)) = (
					thousand_char.to_digit(10),
					decimal_char,
					hundred_char.to_digit(10),
					ten_char.to_digit(10),
				) {
					let mhz = (thousand * 1000 + hundred * 100 + ten * 10) as u16;
					return self.set_detected_cpu_frequency(mhz, CpuFrequencySources::CpuIdTscInfo);
				}
			}
		}

		Err(())
	}

	fn detect_from_hypervisor(&mut self) -> Result<(), ()> {
		fn detect_from_uhyve() -> Result<u16, ()> {
			match boot_info().platform_info {
				PlatformInfo::Uhyve { cpu_freq, .. } => Ok(u16::try_from(
					cpu_freq.map(NonZeroU32::get).unwrap_or_default() / 1000,
				)
				.unwrap()),
				_ => Err(()),
			}
		}
		// future implementations could add support for different hypervisors
		// by adding or_else here
		self.set_detected_cpu_frequency(detect_from_uhyve()?, CpuFrequencySources::Hypervisor)
	}

	extern "x86-interrupt" fn measure_frequency_timer_handler(
		_stack_frame: interrupts::ExceptionStackFrame,
	) {
		MEASUREMENT_TIMER_TICKS.fetch_add(1, Ordering::Relaxed);
		pic::eoi(pit::PIT_INTERRUPT_NUMBER);
	}

	#[cfg(not(target_os = "none"))]
	fn measure_frequency(&mut self) -> Result<(), ()> {
		// return just Ok because the real implementation must run in ring 0
		self.source = CpuFrequencySources::Measurement;
		Ok(())
	}

	#[cfg(target_os = "none")]
	fn measure_frequency(&mut self) -> Result<(), ()> {
		use crate::arch::x86_64::kernel::interrupts::IDT;

		// The PIC is not initialized for uhyve, so we cannot measure anything.
		if env::is_uhyve() {
			return Err(());
		}

		// Measure the CPU frequency by counting 3 ticks of a 100Hz timer.
		let tick_count = 3;
		let measurement_frequency = 100;

		// Use the Programmable Interval Timer (PIT) for this measurement, which is the only
		// system timer with a known constant frequency.
		unsafe {
			let mut idt = IDT.lock();
			idt[pit::PIT_INTERRUPT_NUMBER]
				.set_handler_fn(Self::measure_frequency_timer_handler)
				.set_stack_index(0);
		}
		pit::init(measurement_frequency);

		// we need a timer interrupt to meature the frequency
		interrupts::enable();

		// Determine the current timer tick.
		// We are probably loading this value in the middle of a time slice.
		let first_tick = MEASUREMENT_TIMER_TICKS.load(Ordering::Relaxed);
		let start = get_timestamp();

		// Wait until the tick count changes.
		// As soon as it has done, we are at the start of a new time slice.
		let start_tick = loop {
			let tick = MEASUREMENT_TIMER_TICKS.load(Ordering::Relaxed);
			if tick != first_tick {
				break Some(tick);
			}

			if get_timestamp() - start > 120000000 {
				break None;
			}

			spin_loop();
		}
		.ok_or_else(|| {
			interrupts::disable();
			pit::deinit();
		})?;

		// Count the number of CPU cycles during 3 timer ticks.
		let start = get_timestamp();

		loop {
			let tick = MEASUREMENT_TIMER_TICKS.load(Ordering::Relaxed);
			if tick - start_tick >= tick_count {
				break;
			}

			spin_loop();
		}

		let end = get_timestamp();

		// we don't longer need a timer interrupt
		interrupts::disable();

		// Deinitialize the PIT again.
		// Now we can calculate our CPU frequency and implement a constant frequency tick counter
		// using RDTSC timestamps.
		pit::deinit();

		// Calculate the CPU frequency out of this measurement.
		let cycle_count = end - start;
		let mhz = (measurement_frequency * cycle_count / (1_000_000 * tick_count)) as u16;
		self.set_detected_cpu_frequency(mhz, CpuFrequencySources::Measurement)
	}

	unsafe fn detect(&mut self) {
		let cpuid = CpuId::new();
		unsafe {
			self.detect_from_cpuid(&cpuid)
				.or_else(|_e| self.detect_from_cpuid_tsc_info(&cpuid))
				.or_else(|_e| self.detect_from_cpuid_hypervisor_info(&cpuid))
				.or_else(|_e| self.detect_from_hypervisor())
				.or_else(|_e| self.detect_from_cmdline())
				.or_else(|_e| self.detect_from_cpuid_brand_string(&cpuid))
				.or_else(|_e| self.measure_frequency())
				.or_else(|_e| {
					warn!(
						"Could not determine the processor frequency! Guess a frequency of 2Ghz!"
					);
					self.set_detected_cpu_frequency(2000, CpuFrequencySources::Visionary)
				})
				.unwrap();
		}
	}

	fn get(&self) -> u16 {
		self.mhz
	}
}

impl fmt::Display for CpuFrequency {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "{} MHz (from {})", self.mhz, self.source)
	}
}

struct CpuFeaturePrinter {
	feature_info: FeatureInfo,
	extended_feature_info: ExtendedFeatures,
	extend_processor_identifiers: ExtendedProcessorFeatureIdentifiers,
}

impl CpuFeaturePrinter {
	fn new(cpuid: &CpuId) -> Self {
		CpuFeaturePrinter {
			feature_info: cpuid
				.get_feature_info()
				.expect("CPUID Feature Info not available!"),
			extended_feature_info: cpuid
				.get_extended_feature_info()
				.expect("CPUID Extended Feature Info not available!"),
			extend_processor_identifiers: cpuid
				.get_extended_processor_and_feature_identifiers()
				.expect("Extended Processor and Processor Feature Identifiers not available"),
		}
	}
}

impl fmt::Display for CpuFeaturePrinter {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		if self.feature_info.has_mmx() {
			write!(f, "MMX ")?;
		}
		if self.feature_info.has_sse() {
			write!(f, "SSE ")?;
		}
		if self.feature_info.has_sse2() {
			write!(f, "SSE2 ")?;
		}
		if self.feature_info.has_sse3() {
			write!(f, "SSE3 ")?;
		}
		if self.feature_info.has_ssse3() {
			write!(f, "SSSE3 ")?;
		}
		if self.feature_info.has_sse41() {
			write!(f, "SSE4.1 ")?;
		}
		if self.feature_info.has_sse42() {
			write!(f, "SSE4.2 ")?;
		}
		if self.feature_info.has_avx() {
			write!(f, "AVX ")?;
		}
		if self.feature_info.has_eist() {
			write!(f, "EIST ")?;
		}
		if self.feature_info.has_aesni() {
			write!(f, "AESNI ")?;
		}
		if self.feature_info.has_rdrand() {
			write!(f, "RDRAND ")?;
		}
		if self.feature_info.has_fma() {
			write!(f, "FMA ")?;
		}
		if self.feature_info.has_movbe() {
			write!(f, "MOVBE ")?;
		}
		if self.feature_info.has_mce() {
			write!(f, "MCE ")?;
		}
		if self.feature_info.has_fxsave_fxstor() {
			write!(f, "FXSR ")?;
		}
		if self.feature_info.has_xsave() {
			write!(f, "XSAVE ")?;
		}
		if self.feature_info.has_vmx() {
			write!(f, "VMX ")?;
		}
		if self.extend_processor_identifiers.has_rdtscp() {
			write!(f, "RDTSCP ")?;
		}
		if self.feature_info.has_monitor_mwait() {
			write!(f, "MWAIT ")?;
		}
		if self.extend_processor_identifiers.has_monitorx_mwaitx() {
			write!(f, "MWAITX ")?;
		}
		if self.feature_info.has_clflush() {
			write!(f, "CLFLUSH ")?;
		}
		if self.feature_info.has_dca() {
			write!(f, "DCA ")?;
		}
		if self.feature_info.has_tsc_deadline() {
			write!(f, "TSC-DEADLINE ")?;
		}
		if self.feature_info.has_x2apic() {
			write!(f, "X2APIC ")?;
		}
		if self.feature_info.has_hypervisor() {
			write!(f, "HYPERVISOR ")?;
		}
		if self.extended_feature_info.has_avx2() {
			write!(f, "AVX2 ")?;
		}
		if self.extended_feature_info.has_avx512f() {
			write!(f, "AVX512F ")?;
		}
		if self.extended_feature_info.has_avx512dq() {
			write!(f, "AVX512DQ ")?;
		}
		if self.extended_feature_info.has_avx512_ifma() {
			write!(f, "AVX512IFMA ")?;
		}
		if self.extended_feature_info.has_avx512pf() {
			write!(f, "AVX512PF ")?;
		}
		if self.extended_feature_info.has_avx512er() {
			write!(f, "AVX512ER ")?;
		}
		if self.extended_feature_info.has_avx512cd() {
			write!(f, "AVX512CD ")?;
		}
		if self.extended_feature_info.has_avx512bw() {
			write!(f, "AVX512BW ")?;
		}
		if self.extended_feature_info.has_avx512vl() {
			write!(f, "AVX512VL ")?;
		}
		if self.extended_feature_info.has_bmi1() {
			write!(f, "BMI1 ")?;
		}
		if self.extended_feature_info.has_bmi2() {
			write!(f, "BMI2 ")?;
		}
		if self.extended_feature_info.has_rtm() {
			write!(f, "RTM ")?;
		}
		if self.extended_feature_info.has_hle() {
			write!(f, "HLE ")?;
		}
		if self.extended_feature_info.has_mpx() {
			write!(f, "MPX ")?;
		}
		if self.extended_feature_info.has_pku() {
			write!(f, "PKU ")?;
		}
		if self.extended_feature_info.has_ospke() {
			write!(f, "OSPKE ")?;
		}
		if self.extended_feature_info.has_fsgsbase() {
			write!(f, "FSGSBASE ")?;
		}
		if self.extended_feature_info.has_sgx() {
			write!(f, "SGX ")?;
		}
		if self.extended_feature_info.has_rdseed() {
			write!(f, "RDSEED ")?;
		}

		Ok(())
	}
}

pub(crate) fn run_on_hypervisor() -> bool {
	env::is_uhyve() || FEATURES.run_on_hypervisor
}

#[derive(Debug)]
struct CpuSpeedStep {
	eist_available: bool,
	eist_enabled: bool,
	eist_locked: bool,
	energy_bias_preference: bool,
	max_pstate: u8,
	is_turbo_pstate: bool,
}

impl CpuSpeedStep {
	const fn new() -> Self {
		CpuSpeedStep {
			eist_available: false,
			eist_enabled: false,
			eist_locked: false,
			energy_bias_preference: false,
			max_pstate: 0,
			is_turbo_pstate: false,
		}
	}

	fn detect_features(&mut self, cpuid: &CpuId) {
		let feature_info = cpuid
			.get_feature_info()
			.expect("CPUID Feature Info not available!");

		self.eist_available = feature_info.has_eist();
		if !self.eist_available {
			return;
		}

		let misc = unsafe { rdmsr(IA32_MISC_ENABLE) };
		self.eist_enabled = (misc & IA32_MISC_ENABLE_ENHANCED_SPEEDSTEP) > 0;
		self.eist_locked = (misc & IA32_MISC_ENABLE_SPEEDSTEP_LOCK) > 0;
		if !self.eist_enabled || self.eist_locked {
			return;
		}

		self.max_pstate = (unsafe { rdmsr(MSR_PLATFORM_INFO) } >> 8) as u8;
		if (misc & IA32_MISC_ENABLE_TURBO_DISABLE) == 0 {
			let turbo_pstate = unsafe { rdmsr(MSR_TURBO_RATIO_LIMIT) } as u8;
			if turbo_pstate > self.max_pstate {
				self.max_pstate = turbo_pstate;
				self.is_turbo_pstate = true;
			}
		}

		if let Some(thermal_power_info) = cpuid.get_thermal_power_info() {
			self.energy_bias_preference = thermal_power_info.has_energy_bias_pref();
		}
	}

	fn configure(&self) {
		if !self.eist_available || !self.eist_enabled || self.eist_locked {
			return;
		}

		if self.energy_bias_preference {
			unsafe {
				wrmsr(IA32_ENERGY_PERF_BIAS, 0);
			}
		}

		let mut perf_ctl_mask = u64::from(self.max_pstate) << 8;
		if self.is_turbo_pstate {
			perf_ctl_mask |= 1 << 32;
		}

		unsafe {
			wrmsr(IA32_PERF_CTL, perf_ctl_mask);
		}
	}
}

impl fmt::Display for CpuSpeedStep {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		if self.eist_available {
			write!(f, "Available, ")?;

			if !self.eist_enabled {
				write!(f, "but disabled")?;
			} else if self.eist_locked {
				write!(f, "but locked")?;
			} else {
				write!(f, "enabled with maximum P-State {}", self.max_pstate)?;
				if self.is_turbo_pstate {
					write!(f, " (Turbo Mode)")?;
				}

				if self.energy_bias_preference {
					write!(f, ", disabled Performance/Energy Bias")?;
				}
			}
		} else {
			write!(f, "Not Available")?;
		}

		Ok(())
	}
}

pub fn detect_features() {
	Lazy::force(&FEATURES);
}

pub fn configure() {
	let cpuid = CpuId::new();

	// setup MSR EFER
	unsafe {
		wrmsr(IA32_EFER, rdmsr(IA32_EFER) | EFER_LMA | EFER_SCE | EFER_NXE);
	}

	//
	// CR0 CONFIGURATION
	//
	let mut cr0 = unsafe { cr0() };

	// Enable the FPU.
	cr0.insert(Cr0::CR0_MONITOR_COPROCESSOR | Cr0::CR0_NUMERIC_ERROR);
	cr0.remove(Cr0::CR0_EMULATE_COPROCESSOR);

	// if set, the first FPU access will trigger interrupt 7.
	cr0.insert(Cr0::CR0_TASK_SWITCHED);

	// Prevent writes to read-only pages in Ring 0.
	cr0.insert(Cr0::CR0_WRITE_PROTECT);

	debug!("Set CR0 to {:#x}", cr0);
	unsafe {
		cr0_write(cr0);
	}

	//
	// CR4 CONFIGURATION
	//
	let mut cr4 = unsafe { cr4() };

	let has_pge = match cpuid.get_feature_info() {
		Some(finfo) => finfo.has_pge(),
		None => false,
	};

	if has_pge {
		cr4 |= Cr4::CR4_ENABLE_GLOBAL_PAGES;
	}

	// Enable Machine Check Exceptions.
	// No need to check for support here, all x86-64 CPUs support it.
	cr4.insert(Cr4::CR4_ENABLE_MACHINE_CHECK);

	// Enable full SSE support and indicates that the OS saves SSE context using FXSR.
	// No need to check for support here, all x86-64 CPUs support at least SSE2.
	cr4.insert(Cr4::CR4_ENABLE_SSE | Cr4::CR4_UNMASKED_SSE);

	if supports_xsave() {
		// Indicate that the OS saves extended context (AVX, AVX2, MPX, etc.) using XSAVE.
		cr4.insert(Cr4::CR4_ENABLE_OS_XSAVE);
	}

	// Disable Performance-Monitoring Counters
	cr4.remove(Cr4::CR4_ENABLE_PPMC);
	// clear TSD => every privilege level is able
	// to use rdtsc
	cr4.remove(Cr4::CR4_TIME_STAMP_DISABLE);

	if supports_fsgs() {
		cr4.insert(Cr4::CR4_ENABLE_FSGSBASE);
		debug!("Enable FSGSBASE support");
	}
	#[cfg(feature = "fsgsbase")]
	if !supports_fsgs() {
		error!("FSGSBASE support is enabled, but the processor doesn't support it!");
		crate::scheduler::shutdown(1);
	}

	debug!("Set CR4 to {:#x}", cr4);
	unsafe {
		cr4_write(cr4);
	}

	//
	// XCR0 CONFIGURATION
	//
	if supports_xsave() {
		// Enable saving the context for all known vector extensions.
		// Must happen after CR4_ENABLE_OS_XSAVE has been set.
		let mut xcr0 = unsafe { xcr0() };
		xcr0.insert(Xcr0::XCR0_FPU_MMX_STATE | Xcr0::XCR0_SSE_STATE);

		if supports_avx() {
			xcr0.insert(Xcr0::XCR0_AVX_STATE);
		}

		debug!("Set XCR0 to {:#x}", xcr0);
		unsafe {
			xcr0_write(xcr0);
		}
	}

	// enable support of syscall and sysret
	#[cfg(feature = "common-os")]
	unsafe {
		let has_syscall = match cpuid.get_extended_processor_and_feature_identifiers() {
			Some(finfo) => finfo.has_syscall_sysret(),
			None => false,
		};

		if has_syscall {
			info!("Enable SYSCALL support");
		} else {
			panic!("Syscall support is missing");
		}
		wrmsr(IA32_STAR, (0x1Bu64 << 48) | (0x08u64 << 32));
		wrmsr(
			IA32_LSTAR,
			crate::arch::x86_64::kernel::syscall::syscall_handler as u64,
		);
		wrmsr(IA32_FMASK, 1 << 9); // clear IF flag during system call
	}

	// Initialize the FS register, which is later used for Thread-Local Storage.
	writefs(0);

	//
	// ENHANCED INTEL SPEEDSTEP CONFIGURATION
	//
	FEATURES.cpu_speedstep.configure();
}

pub fn detect_frequency() {
	Lazy::force(&CPU_FREQUENCY);
}

pub fn print_information() {
	infoheader!(" CPU INFORMATION ");

	let cpuid = CpuId::new();
	let feature_printer = CpuFeaturePrinter::new(&cpuid);

	if let Some(brand_string) = cpuid.get_processor_brand_string() {
		infoentry!("Model", brand_string.as_str());
	}

	infoentry!("Frequency", *CPU_FREQUENCY);
	infoentry!("SpeedStep Technology", FEATURES.cpu_speedstep);

	infoentry!("Features", feature_printer);
	infoentry!(
		"Physical Address Width",
		"{} bits",
		get_physical_address_bits()
	);
	infoentry!("Linear Address Width", "{} bits", get_linear_address_bits());
	infoentry!(
		"Supports 1GiB Pages",
		if supports_1gib_pages() { "Yes" } else { "No" }
	);
	infofooter!();
}

pub fn seed_entropy() -> Option<[u8; 32]> {
	let mut buf = [0; 32];
	if FEATURES.supports_rdseed {
		for word in buf.chunks_mut(8) {
			let mut value = 0;

			// Some RDRAND implementations on AMD CPUs have had bugs where the carry
			// flag was incorrectly set without there actually being a random value
			// available. Even though no bugs are known for RDSEED, we should not
			// consider the default values random for extra security.
			while unsafe { _rdseed64_step(&mut value) != 1 } || value == 0 || value == u64::MAX {
				// Spin as per the recommendation in the
				// Intel® Digital Random Number Generator (DRNG) implementation guide
				spin_loop();
			}

			word.copy_from_slice(&value.to_ne_bytes());
		}

		Some(buf)
	} else {
		None
	}
}

#[inline]
pub fn get_linear_address_bits() -> u8 {
	FEATURES.linear_address_bits
}

#[inline]
pub fn get_physical_address_bits() -> u8 {
	FEATURES.physical_address_bits
}

#[inline]
pub fn supports_1gib_pages() -> bool {
	FEATURES.supports_1gib_pages
}

#[inline]
pub fn supports_2mib_pages() -> bool {
	true
}

#[inline]
pub fn supports_avx() -> bool {
	FEATURES.supports_avx
}

#[inline]
pub fn supports_tsc_deadline() -> bool {
	FEATURES.supports_tsc_deadline
}

#[inline]
pub fn supports_x2apic() -> bool {
	FEATURES.supports_x2apic
}

#[inline]
pub fn supports_xsave() -> bool {
	FEATURES.supports_xsave
}

#[inline]
pub fn supports_mwait() -> bool {
	FEATURES.supports_mwait
}

#[inline]
pub fn supports_clflush() -> bool {
	FEATURES.supports_clflush
}

#[inline]
pub fn supports_fsgs() -> bool {
	FEATURES.supports_fsgs
}

/// The halt function stops the processor until the next interrupt arrives
pub fn halt() {
	unsafe {
		x86::halt();
	}
}

/// Causes a triple fault.
///
/// Triple faults cause CPU resets.
/// On KVM, this results in `KVM_EXIT_SHUTDOWN`.
/// This is the preferred way of shutting down the CPU on firecracker and in QEMU's `microvm` virtual platform.
///
/// See [Triple Faulting the CPU](http://www.rcollins.org/Productivity/TripleFault.html).
fn triple_fault() -> ! {
	let idt = DescriptorTablePointer {
		limit: 0,
		base: VirtAddr::zero(),
	};
	unsafe { lidt(&idt) };
	int3();
	unreachable!()
}

fn qemu_exit(success: bool) {
	let code = if success { 3 >> 1 } else { 0 };
	unsafe {
		Port::<u32>::new(0xf4).write(code);
	}
}

/// Shutdown the system
pub fn shutdown(error_code: i32) -> ! {
	qemu_exit(error_code == 0);

	#[cfg(feature = "acpi")]
	{
		acpi::poweroff();
	}

	triple_fault()
}

pub fn get_timer_ticks() -> u64 {
	// We simulate a timer with a 1 microsecond resolution by taking the CPU timestamp
	// and dividing it by the CPU frequency in MHz.
	get_timestamp() / u64::from(get_frequency())
}

pub fn get_frequency() -> u16 {
	CPU_FREQUENCY.get()
}

#[inline]
pub fn readfs() -> usize {
	if cfg!(feature = "fsgsbase") {
		unsafe { segmentation::rdfsbase() }
	} else {
		unsafe { rdmsr(IA32_GS_BASE) }
	}
	.try_into()
	.unwrap()
}

#[inline]
pub fn readgs() -> usize {
	if cfg!(feature = "fsgsbase") {
		unsafe { segmentation::rdgsbase() }
	} else {
		unsafe { rdmsr(IA32_FS_BASE) }
	}
	.try_into()
	.unwrap()
}

#[inline]
pub fn writefs(fs: usize) {
	let fs = fs.try_into().unwrap();
	if cfg!(feature = "fsgsbase") {
		unsafe { segmentation::wrfsbase(fs) }
	} else {
		unsafe { wrmsr(IA32_FS_BASE, fs) }
	}
}

#[inline]
pub fn writegs(gs: usize) {
	let gs = gs.try_into().unwrap();
	if cfg!(feature = "fsgsbase") {
		unsafe { segmentation::wrgsbase(gs) }
	} else {
		unsafe { wrmsr(IA32_GS_BASE, gs) }
	}
}

#[inline]
pub fn get_timestamp() -> u64 {
	unsafe {
		if FEATURES.supports_rdtscp {
			get_timestamp_rdtscp()
		} else {
			get_timestamp_rdtsc()
		}
	}
}

unsafe fn get_timestamp_rdtsc() -> u64 {
	unsafe {
		_mm_lfence();
		let value = _rdtsc();
		_mm_lfence();
		value
	}
}

unsafe fn get_timestamp_rdtscp() -> u64 {
	unsafe {
		let mut aux: u32 = 0;
		let value = __rdtscp(&mut aux);
		_mm_lfence();
		value
	}
}

/// Delay execution by the given number of microseconds using busy-waiting.
#[inline]
pub fn udelay(usecs: u64) {
	let end = get_timestamp() + u64::from(get_frequency()) * usecs;
	while get_timestamp() < end {
		spin_loop();
	}
}
