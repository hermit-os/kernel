#![allow(dead_code)]

use alloc::boxed::Box;
use core::arch::asm;
use core::arch::x86_64::{
	__rdtscp, _fxrstor, _fxsave, _mm_lfence, _rdseed64_step, _rdtsc, _xrstor, _xsave, _xsavec,
	_xsaveopt,
};
use core::fmt;
use core::hint::spin_loop;
use core::num::{NonZero, NonZeroU32};
use core::sync::atomic::{AtomicU64, Ordering};

use hermit_entry::boot_info::PlatformInfo;
use hermit_sync::Lazy;
use raw_cpuid::*;
use x86_64::instructions::interrupts::int3;
use x86_64::instructions::port::Port;
use x86_64::instructions::tables::lidt;
use x86_64::registers::control::{Cr0, Cr0Flags, Cr4, Cr4Flags, Efer, EferFlags};
use x86_64::registers::model_specific::{FsBase, GsBase, Msr};
use x86_64::registers::mxcsr::MxCsr;
use x86_64::registers::segmentation::{FS, GS, Segment64};
use x86_64::registers::xcontrol::{XCr0, XCr0Flags};
use x86_64::structures::DescriptorTablePointer;
use x86_64::{VirtAddr, instructions};

#[cfg(feature = "acpi")]
use crate::arch::x86_64::kernel::acpi;
use crate::arch::x86_64::kernel::{interrupts, pic, pit};
use crate::env;

/// see <http://biosbits.org>.
const MSR_PLATFORM_INFO: u32 = 0xce;

/// See Table 35-2. See Section 14.1, Enhanced Intel  Speedstep® Technology.
const IA32_PERF_CTL: u32 = 0x199;

const IA32_MISC_ENABLE: u32 = 0x1a0;

const IA32_MISC_ENABLE_ENHANCED_SPEEDSTEP: u64 = 1 << 16;
const IA32_MISC_ENABLE_SPEEDSTEP_LOCK: u64 = 1 << 20;
const IA32_MISC_ENABLE_TURBO_DISABLE: u64 = 1 << 38;

/// Maximum Ratio Limit of Turbo Mode RO if MSR_PLATFORM_INFO.\[28\] = 0, RW if MSR_PLATFORM_INFO.\[28\] = 1
const MSR_TURBO_RATIO_LIMIT: u32 = 0x1ad;

/// if CPUID.6H:ECX\[3\] = 1
const IA32_ENERGY_PERF_BIAS: u32 = 0x1b0;

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
	has_xsaveopt: bool,
	has_xsavec: bool,
	xcr0_supports_avx512_opmask: bool,
	xcr0_supports_avx512_zmm_hi16: bool,
	xcr0_supports_avx512_zmm_hi256: bool,
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
	let extended_state_info = cpuid
		.get_extended_state_info()
		.expect("CPUID Extended state info not available");

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
		has_xsaveopt: extended_state_info.has_xsaveopt(),
		has_xsavec: extended_state_info.has_xsavec(),
		xcr0_supports_avx512_opmask: extended_state_info.xcr0_supports_avx512_opmask(),
		xcr0_supports_avx512_zmm_hi16: extended_state_info.xcr0_supports_avx512_zmm_hi16(),
		xcr0_supports_avx512_zmm_hi256: extended_state_info.xcr0_supports_avx512_zmm_hi256(),
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
	pub mxcsr: MxCsr,
	pub mxcsr_mask: u32,
	pub st_space: [u8; 8 * 16],
	pub xmm_space: [u8; 16 * 16],
	pub padding: [u8; 96],
}

#[derive(Clone)]
#[repr(C, align(64))]
struct AlignToSixtyFour([u8; 64]);

#[repr(C)]
pub struct FPUState {
	xsave_area: Box<[AlignToSixtyFour]>,
}

impl FPUState {
	pub fn new() -> Self {
		let xsave_size = if supports_xsave() {
			CpuId::new()
				.get_extended_state_info()
				.expect("XSAVE requires extended state info")
				.xsave_area_size_enabled_features() as usize
		} else {
			size_of::<XSaveLegacyRegion>()
		};

		debug!("XSAVE area size: {xsave_size}");

		// Allocate a 64-byte aligned Vec
		let n_units = xsave_size.div_ceil(size_of::<AlignToSixtyFour>());
		let mut xsave_area = vec![AlignToSixtyFour([0; 64]); n_units].into_boxed_slice();

		// SAFETY: We allocated at least the size of XSaveLegacyRegion bytes and have initialized them
		let legacy_region = unsafe { &mut *xsave_area.as_mut_ptr().cast::<XSaveLegacyRegion>() };

		// Set FPU-related values to their default values after initialization.
		// Refer to Intel Vol. 3A, Table 9-1. IA-32 and Intel 64 Processor States Following Power-up, Reset, or INIT
		legacy_region.fpu_control_word = 0x37f;
		legacy_region.fpu_tag_word = 0xffff;
		legacy_region.mxcsr = MxCsr::default();

		Self { xsave_area }
	}

	pub fn restore(&self) {
		if supports_xsave() {
			unsafe {
				_xrstor(self.xsave_area.as_ptr().cast::<u8>(), u64::MAX);
			}
		} else {
			self.restore_common();
		}
	}

	pub fn save(&mut self) {
		if supports_xsave() {
			if has_xsavec() {
				unsafe { _xsavec(self.xsave_area.as_mut_ptr().cast::<u8>(), u64::MAX) }
			} else if has_xsaveopt() {
				unsafe { _xsaveopt(self.xsave_area.as_mut_ptr().cast::<u8>(), u64::MAX) }
			} else {
				unsafe { _xsave(self.xsave_area.as_mut_ptr().cast::<u8>(), u64::MAX) }
			}
		} else {
			self.save_common();
		}
	}

	pub fn restore_common(&self) {
		unsafe {
			_fxrstor(self.xsave_area.as_ptr().cast::<u8>());
		}
	}

	pub fn save_common(&mut self) {
		unsafe {
			_fxsave(self.xsave_area.as_mut_ptr().cast::<u8>());
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
	Fdt,
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
			CpuFrequencySources::Invalid => {
				panic!("Attempted to print an invalid CPU Frequency Source")
			}
			CpuFrequencySources::Fdt => write!(f, "FDT"),
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

	unsafe fn detect_from_cpuid(&mut self, cpuid: &CpuId<CpuIdReaderNative>) -> Result<(), ()> {
		let processor_frequency_info = cpuid.get_processor_frequency_info();

		match processor_frequency_info {
			Some(freq_info) => {
				let mhz = freq_info.processor_base_frequency();
				self.set_detected_cpu_frequency(mhz, CpuFrequencySources::CpuId)
			}
			None => Err(()),
		}
	}

	unsafe fn detect_from_cpuid_tsc_info(
		&mut self,
		cpuid: &CpuId<CpuIdReaderNative>,
	) -> Result<(), ()> {
		let tsc_info = cpuid.get_tsc_info().ok_or(())?;
		let freq = tsc_info.tsc_frequency().ok_or(())?;
		let mhz = (freq / 1_000_000u64) as u16;
		self.set_detected_cpu_frequency(mhz, CpuFrequencySources::CpuIdTscInfo)
	}

	unsafe fn detect_from_cpuid_hypervisor_info(
		&mut self,
		cpuid: &CpuId<CpuIdReaderNative>,
	) -> Result<(), ()> {
		const KHZ_TO_HZ: u64 = 1000;
		const MHZ_TO_HZ: u64 = 1_000_000;
		let hypervisor_info = cpuid.get_hypervisor_info().ok_or(())?;
		let freq = u64::from(hypervisor_info.tsc_frequency().ok_or(())?) * KHZ_TO_HZ;
		let mhz: u16 = (freq / MHZ_TO_HZ).try_into().unwrap();
		self.set_detected_cpu_frequency(mhz, CpuFrequencySources::HypervisorTscInfo)
	}

	unsafe fn detect_from_cpuid_brand_string(
		&mut self,
		cpuid: &CpuId<CpuIdReaderNative>,
	) -> Result<(), ()> {
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
					return self
						.set_detected_cpu_frequency(mhz, CpuFrequencySources::CpuIdBrandString);
				}
			}
		}

		Err(())
	}

	fn detect_from_fdt(&mut self) -> Result<(), ()> {
		fn mhz_from_fdt() -> Option<NonZero<u16>> {
			let khz = env::fdt()?
				.find_node("/hermit,tsc")?
				.property("khz")?
				.as_usize()?;
			let khz = u32::try_from(khz).ok()?;
			let mhz = u16::try_from(khz / 1000).ok()?;
			NonZero::new(mhz)
		}

		let mhz = mhz_from_fdt().ok_or(())?;
		self.set_detected_cpu_frequency(mhz.get(), CpuFrequencySources::Fdt)?;

		Ok(())
	}

	fn detect_from_hypervisor(&mut self) -> Result<(), ()> {
		fn detect_from_uhyve() -> Result<u16, ()> {
			match env::boot_info().platform_info {
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

			if get_timestamp() - start > 120_000_000 {
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
			self.detect_from_fdt()
				.or_else(|_e| self.detect_from_cpuid(&cpuid))
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
	fn new(cpuid: &CpuId<CpuIdReaderNative>) -> Self {
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

	fn detect_features(&mut self, cpuid: &CpuId<CpuIdReaderNative>) {
		let feature_info = cpuid
			.get_feature_info()
			.expect("CPUID Feature Info not available!");

		self.eist_available = feature_info.has_eist();
		if !self.eist_available {
			return;
		}

		let misc = unsafe { Msr::new(IA32_MISC_ENABLE).read() };
		self.eist_enabled = (misc & IA32_MISC_ENABLE_ENHANCED_SPEEDSTEP) > 0;
		self.eist_locked = (misc & IA32_MISC_ENABLE_SPEEDSTEP_LOCK) > 0;
		if !self.eist_enabled || self.eist_locked {
			return;
		}

		self.max_pstate = (unsafe { Msr::new(MSR_PLATFORM_INFO).read() } >> 8) as u8;
		if (misc & IA32_MISC_ENABLE_TURBO_DISABLE) == 0 {
			let turbo_pstate = unsafe { Msr::new(MSR_TURBO_RATIO_LIMIT).read() } as u8;
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
				Msr::new(IA32_ENERGY_PERF_BIAS).write(0);
			}
		}

		let mut perf_ctl_mask = u64::from(self.max_pstate) << 8;
		if self.is_turbo_pstate {
			perf_ctl_mask |= 1 << 32;
		}

		unsafe {
			Msr::new(IA32_PERF_CTL).write(perf_ctl_mask);
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
		Efer::update(|flags| {
			flags.insert(
				EferFlags::SYSTEM_CALL_EXTENSIONS
					| EferFlags::LONG_MODE_ACTIVE
					| EferFlags::NO_EXECUTE_ENABLE,
			);
		});
	}

	//
	// CR0 CONFIGURATION
	//
	unsafe {
		Cr0::update(|flags| {
			// Enable the FPU.
			flags.insert(Cr0Flags::MONITOR_COPROCESSOR | Cr0Flags::NUMERIC_ERROR);
			flags.remove(Cr0Flags::EMULATE_COPROCESSOR);

			// if set, the first FPU access will trigger interrupt 7.
			flags.insert(Cr0Flags::TASK_SWITCHED);

			// Prevent writes to read-only pages in Ring 0.
			flags.insert(Cr0Flags::WRITE_PROTECT);

			debug!("Setting CR0 = {flags:?}");
		});
	}

	//
	// CR4 CONFIGURATION
	//
	unsafe {
		Cr4::update(|flags| {
			let has_pge = cpuid
				.get_feature_info()
				.is_some_and(|feature_info| feature_info.has_pge());

			if has_pge {
				flags.insert(Cr4Flags::PAGE_GLOBAL);
			}

			// Enable Machine Check Exceptions.
			// No need to check for support here, all x86-64 CPUs support it.
			flags.insert(Cr4Flags::MACHINE_CHECK_EXCEPTION);

			// Enable full SSE support and indicates that the OS saves SSE context using FXSR.
			// No need to check for support here, all x86-64 CPUs support at least SSE2.
			flags.insert(Cr4Flags::OSFXSR | Cr4Flags::OSXMMEXCPT_ENABLE);

			if supports_xsave() {
				// Indicate that the OS saves extended context (AVX, AVX2, MPX, etc.) using XSAVE.
				flags.insert(Cr4Flags::OSXSAVE);
			}

			// Disable Performance-Monitoring Counters
			flags.remove(Cr4Flags::PERFORMANCE_MONITOR_COUNTER);
			// clear TSD => every privilege level is able
			// to use rdtsc
			flags.remove(Cr4Flags::TIMESTAMP_DISABLE);

			if supports_fsgs() {
				flags.insert(Cr4Flags::FSGSBASE);
				debug!("Enable FSGSBASE support");
			}
			#[cfg(feature = "fsgsbase")]
			if !supports_fsgs() {
				error!("FSGSBASE support is enabled, but the processor doesn't support it!");
				crate::scheduler::shutdown(1);
			}

			debug!("Setting CR4 = {flags:?}");
		});
	}

	//
	// XCR0 CONFIGURATION
	//
	if supports_xsave() {
		// Enable saving the context for all known vector extensions.
		// Must happen after CR4_ENABLE_OS_XSAVE has been set.
		// FIXME: migrate to `XCr0::update()` once available:
		// https://github.com/rust-osdev/x86_64/pull/527
		let mut flags = XCr0::read();
		flags.insert(XCr0Flags::X87 | XCr0Flags::SSE);

		if supports_avx() {
			flags.insert(XCr0Flags::AVX);
		}

		if xcr0_supports_avx512_opmask() {
			flags.insert(XCr0Flags::OPMASK);
		}

		if xcr0_supports_avx512_zmm_hi16() {
			flags.insert(XCr0Flags::HI16_ZMM);
		}

		if xcr0_supports_avx512_zmm_hi256() {
			flags.insert(XCr0Flags::ZMM_HI256);
		}

		debug!("Setting XCR0 = {flags:?}");
		unsafe {
			XCr0::write(flags);
		}
	}

	// enable support of syscall and sysret
	#[cfg(feature = "common-os")]
	{
		use x86_64::PrivilegeLevel;
		use x86_64::registers::model_specific::{LStar, SFMask, Star};
		use x86_64::registers::rflags::RFlags;
		use x86_64::structures::gdt::SegmentSelector;

		use crate::arch::x86_64::kernel::syscall;

		let has_syscall = match cpuid.get_extended_processor_and_feature_identifiers() {
			Some(finfo) => finfo.has_syscall_sysret(),
			None => false,
		};

		if has_syscall {
			info!("Enable SYSCALL support");
		} else {
			panic!("Syscall support is missing");
		}
		let cs_sysret = SegmentSelector::new(5, PrivilegeLevel::Ring3);
		let ss_sysret = SegmentSelector::new(4, PrivilegeLevel::Ring3);
		let cs_syscall = SegmentSelector::new(1, PrivilegeLevel::Ring0);
		let ss_syscall = SegmentSelector::new(2, PrivilegeLevel::Ring0);
		Star::write(cs_sysret, ss_sysret, cs_syscall, ss_syscall).unwrap();
		let syscall_handler_addr = syscall::syscall_handler as *const ();
		let syscall_handler_addr = VirtAddr::from_ptr(syscall_handler_addr);
		LStar::write(syscall_handler_addr);
		SFMask::write(RFlags::INTERRUPT_FLAG); // clear IF flag during system call
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
		let brand_string = brand_string.as_str();
		infoentry!("Model", "{brand_string}");
	}

	let cpu_freq = &*CPU_FREQUENCY;
	let speedstep = &FEATURES.cpu_speedstep;
	infoentry!("Frequency", "{cpu_freq}");
	infoentry!("SpeedStep Technology", "{speedstep}");

	infoentry!("Features", "{feature_printer}");

	let phys_addr_bits = get_physical_address_bits();
	let virt_addr_bits = get_linear_address_bits();
	let size_1gib_pages = if supports_1gib_pages() { "Yes" } else { "No" };
	infoentry!("Physical Address Width", "{phys_addr_bits} bits");
	infoentry!("Linear Address Width", "{virt_addr_bits} bits");
	infoentry!("Supports 1GiB Pages", "{size_1gib_pages}");

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

#[inline]
pub fn has_xsaveopt() -> bool {
	FEATURES.has_xsaveopt
}

#[inline]
pub fn has_xsavec() -> bool {
	FEATURES.has_xsavec
}

#[inline]
pub fn xcr0_supports_avx512_opmask() -> bool {
	FEATURES.xcr0_supports_avx512_opmask
}

#[inline]
pub fn xcr0_supports_avx512_zmm_hi16() -> bool {
	FEATURES.xcr0_supports_avx512_zmm_hi16
}

#[inline]
pub fn xcr0_supports_avx512_zmm_hi256() -> bool {
	FEATURES.xcr0_supports_avx512_zmm_hi256
}

/// The halt function stops the processor until the next interrupt arrives
pub fn halt() {
	instructions::hlt();
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

/// Writes an exit code into the isa-debug-exit port.
///
/// For a value `e` written into the port, QEMU will exit with `(e << 1) | 1`.
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

/// Returns the timer frequency in MHz
pub fn get_frequency() -> u16 {
	CPU_FREQUENCY.get()
}

#[inline]
pub fn readfs() -> usize {
	let base = if cfg!(feature = "fsgsbase") {
		FS::read_base()
	} else {
		FsBase::read()
	};

	base.as_u64().try_into().unwrap()
}

#[inline]
pub fn readgs() -> usize {
	let base = if cfg!(feature = "fsgsbase") {
		GS::read_base()
	} else {
		GsBase::read()
	};

	base.as_u64().try_into().unwrap()
}

#[inline]
pub fn writefs(fs: usize) {
	let base = VirtAddr::new(fs.try_into().unwrap());
	if cfg!(feature = "fsgsbase") {
		unsafe {
			FS::write_base(base);
		}
	} else {
		FsBase::write(base);
	}
}

#[inline]
pub fn writegs(gs: usize) {
	let base = VirtAddr::new(gs.try_into().unwrap());
	if cfg!(feature = "fsgsbase") {
		unsafe {
			GS::write_base(base);
		}
	} else {
		GsBase::write(base);
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
		let value = __rdtscp(&raw mut aux);
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
