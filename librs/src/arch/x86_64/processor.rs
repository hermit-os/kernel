// Copyright (c) 2017 Stefan Lankes, RWTH Aachen University
//                    Colin Finck, RWTH Aachen University
//
// MIT License
//
// Permission is hereby granted, free of charge, to any person obtaining
// a copy of this software and associated documentation files (the
// "Software"), to deal in the Software without restriction, including
// without limitation the rights to use, copy, modify, merge, publish,
// distribute, sublicense, and/or sell copies of the Software, and to
// permit persons to whom the Software is furnished to do so, subject to
// the following conditions:
//
// The above copyright notice and this permission notice shall be
// included in all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
// EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
// MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
// NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE
// LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
// OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION
// WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.

use arch::x86_64::percore::*;
use core::{fmt, ptr, slice, str};
use logging::*;
use raw_cpuid::*;
use tasks::*;
use x86::bits64::time::*;
use x86::shared::control_regs::*;
use x86::shared::msr::*;


extern "C" {
	#[link_section = ".percore"]
	static __core_id: u32;

	static cmdline: *const u8;
	static cmdsize: usize;
	static current_boot_id: i32;
	static mut Lpatch0: u8;
	static mut Lpatch1: u8;
	static mut Lpatch2: u8;
	static percore_start: u8;
	static percore_end0: u8;
}

const EFER_NXE: u64 = 1 << 11;


static mut CPU_FREQUENCY: CpuFrequency = CpuFrequency::new();
static mut PHYSICAL_ADDRESS_BITS: u8 = 0;
static mut LINEAR_ADDRESS_BITS: u8 = 0;
static mut SUPPORTS_AVX: bool = false;
static mut SUPPORTS_1GIB_PAGES: bool = false;
static mut SUPPORTS_FSGSBASE: bool = false;
static mut SUPPORTS_XSAVE: bool = false;
static mut TIMESTAMP_FUNCTION: unsafe fn() -> u64 = get_timestamp_rdtsc;


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
	pub st_space: [u8; 8*16],
	pub xmm_space: [u8; 16*16],
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
	pub ymmh_space: [u8; 16*16],
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
	pub bound_registers: [u8; 4*16],
}

#[repr(C)]
pub struct XSaveBndcsr {
	pub bndcfgu_register: u64,
	pub bndstatus_register: u64,
}

#[repr(C, align(64))]
pub struct XSaveArea {
	pub legacy_region: XSaveLegacyRegion,
	pub header: XSaveHeader,
	pub avx_state: XSaveAVXState,
	pub lwp_state: XSaveLWPState,
	pub bndregs: XSaveBndregs,
	pub bndcsr: XSaveBndcsr,
}


enum CpuFrequencySources {
	Invalid,
	CommandLine,
	CpuIdFrequencyInfo,
	CpuIdBrandString,
	Measurement,
}

impl fmt::Display for CpuFrequencySources {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		match self {
			&CpuFrequencySources::CommandLine => write!(f, "Command Line"),
			&CpuFrequencySources::CpuIdFrequencyInfo => write!(f, "CPUID Frequency Info"),
			&CpuFrequencySources::CpuIdBrandString => write!(f, "CPUID Brand String"),
			&CpuFrequencySources::Measurement => write!(f, "Measurement"),
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
		CpuFrequency { mhz: 0, source: CpuFrequencySources::Invalid }
	}

	unsafe fn detect_from_cmdline(&mut self) -> bool {
		if cmdsize > 0 {
			let slice = slice::from_raw_parts(cmdline, cmdsize);
			let cmdline_str = str::from_utf8_unchecked(slice);

			let freq_find = cmdline_str.find("-freq");
			if freq_find.is_some() {
				let cmdline_freq_str = cmdline_str.split_at(freq_find.unwrap() + "-freq".len()).1;
				let mhz_str = cmdline_freq_str.split(' ').next().expect("Invalid -freq command line");

				self.mhz = mhz_str.parse().expect("Could not parse -freq command line as number");
				self.source = CpuFrequencySources::CommandLine;
				true
			} else {
				false
			}
		} else {
			false
		}
	}

	unsafe fn detect_from_cpuid_frequency_info(&mut self, cpuid: &CpuId) -> bool {
		match cpuid.get_processor_frequency_info() {
			Some(info) => {
				self.mhz = info.processor_base_frequency();
				self.source = CpuFrequencySources::CpuIdFrequencyInfo;
				true
			},
			_ => false
		}
	}

	unsafe fn detect_from_cpuid_brand_string(&mut self, cpuid: &CpuId) -> bool {
		let extended_function_info = cpuid.get_extended_function_info().expect("CPUID Extended Function Info not available!");
		let brand_string = extended_function_info.processor_brand_string().expect("CPUID Brand String not available!");

		let ghz_find = brand_string.find("GHz");
		if ghz_find.is_some() {
			let index = ghz_find.unwrap() - 4;
			let thousand = brand_string.chars().nth(index).unwrap();
			let decimal_dot = brand_string.chars().nth(index + 1).unwrap();
			let hundred = brand_string.chars().nth(index + 2).unwrap();
			let ten = brand_string.chars().nth(index + 3).unwrap();

			if thousand.is_digit(10) && decimal_dot == '.' && hundred.is_digit(10) && ten.is_digit(10) {
				self.mhz = (thousand.to_digit(10).unwrap() * 1000 + hundred.to_digit(10).unwrap() * 100 + ten.to_digit(10).unwrap() * 10) as u16;
				self.source = CpuFrequencySources::CpuIdBrandString;
				true
			} else {
				false
			}
		} else {
			false
		}
	}

	unsafe fn measure_frequency(&mut self) -> bool {
		// TODO! Timer needs to be initialized before this can work.
		panic!("measure_frequency not yet implemented!");
		true
	}

	unsafe fn detect(&mut self) {
		let cpuid = CpuId::new();
		self.detect_from_cmdline()
			|| self.detect_from_cpuid_frequency_info(&cpuid)
			|| self.detect_from_cpuid_brand_string(&cpuid)
			|| self.measure_frequency();
	}

	fn get(&self) -> u16 {
		self.mhz
	}
}

impl fmt::Display for CpuFrequency {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		write!(f, "{} MHz (from {})", self.mhz, self.source)
	}
}


struct FeaturePrinter {
	feature_info: FeatureInfo,
	extended_feature_info: ExtendedFeatures,
	extended_function_info: ExtendedFunctionInfo,
}

impl FeaturePrinter {
	fn new(cpuid: &CpuId) -> Self {
		FeaturePrinter {
			feature_info: cpuid.get_feature_info().expect("CPUID Feature Info not available!"),
			extended_feature_info: cpuid.get_extended_feature_info().expect("CPUID Extended Feature Info not available!"),
			extended_function_info: cpuid.get_extended_function_info().expect("CPUID Extended Function Info not available!"),
		}
	}
}

impl fmt::Display for FeaturePrinter {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		if self.feature_info.has_mmx() { write!(f, "MMX ").unwrap(); }
		if self.feature_info.has_sse() { write!(f, "SSE ").unwrap(); }
		if self.feature_info.has_sse2() { write!(f, "SSE2 ").unwrap(); }
		if self.feature_info.has_sse3() { write!(f, "SSE3 ").unwrap(); }
		if self.feature_info.has_ssse3() { write!(f, "SSSE3 ").unwrap(); }
		if self.feature_info.has_sse41() { write!(f, "SSE4.1 ").unwrap(); }
		if self.feature_info.has_sse42() { write!(f, "SSE4.2 ").unwrap(); }
		if self.feature_info.has_avx() { write!(f, "AVX ").unwrap(); }
		if self.extended_feature_info.has_avx2() { write!(f, "AVX2 ").unwrap(); }
		if self.feature_info.has_aesni() { write!(f, "AESNI ").unwrap(); }
		if self.feature_info.has_rdrand() { write!(f, "RDRAND ").unwrap(); }
		if self.feature_info.has_fma() { write!(f, "FMA ").unwrap(); }
		if self.feature_info.has_movbe() { write!(f, "MOVBE ").unwrap(); }
		if self.feature_info.has_x2apic() { write!(f, "X2APIC ").unwrap(); }
		if self.feature_info.has_mce() { write!(f, "MCE ").unwrap(); }
		if self.feature_info.has_fxsave_fxstor() { write!(f, "FXSR ").unwrap(); }
		if self.feature_info.has_xsave() { write!(f, "XSAVE ").unwrap(); }
		if self.feature_info.has_vmx() { write!(f, "VMX ").unwrap(); }
		if self.extended_function_info.has_rdtscp() { write!(f, "RDTSCP ").unwrap(); }
		if self.feature_info.has_monitor_mwait() { write!(f, "MWAIT ").unwrap(); }
		if self.feature_info.has_clflush() { write!(f, "CLFLUSH ").unwrap(); }
		if self.extended_feature_info.has_bmi1() { write!(f, "BMI1 ").unwrap(); }
		if self.extended_feature_info.has_bmi2() { write!(f, "BMI2 ").unwrap(); }
		if self.extended_feature_info.has_fsgsbase() { write!(f, "FSGSBASE ").unwrap(); }
		if self.feature_info.has_dca() { write!(f, "DCA ").unwrap(); }
		if self.extended_feature_info.has_rtm() { write!(f, "RTM ").unwrap(); }
		if self.extended_feature_info.has_hle() { write!(f, "HLE ").unwrap(); }
		if self.extended_feature_info.has_qm() { write!(f, "CQM ").unwrap(); }
		if self.extended_feature_info.has_mpx() { write!(f, "MPX ").unwrap(); }
		Ok(())
	}
}


pub fn detect_features() {
	// Detect CPU features
	let cpuid = CpuId::new();
	let feature_info = cpuid.get_feature_info().expect("CPUID Feature Info not available!");
	let extended_feature_info = cpuid.get_extended_feature_info().expect("CPUID Extended Feature Info not available!");
	let extended_function_info = cpuid.get_extended_function_info().expect("CPUID Extended Function Info not available!");

	unsafe {
		PHYSICAL_ADDRESS_BITS = extended_function_info.physical_address_bits().expect("CPUID Physical Address Bits not available!");
		LINEAR_ADDRESS_BITS = extended_function_info.linear_address_bits().expect("CPUID Linear Address Bits not available!");
		SUPPORTS_AVX = feature_info.has_avx();
		SUPPORTS_1GIB_PAGES = extended_function_info.has_1gib_pages();
		SUPPORTS_FSGSBASE = extended_feature_info.has_fsgsbase();
		SUPPORTS_XSAVE = feature_info.has_xsave();

		if extended_function_info.has_rdtscp() {
			TIMESTAMP_FUNCTION = get_timestamp_rdtscp;
		}
	}
}

pub fn configure() {
	//
	// CR0 CONFIGURATION
	//
	let mut cr0 = unsafe { cr0() };

	// Enable the FPU.
	cr0.insert(CR0_MONITOR_COPROCESSOR | CR0_NUMERIC_ERROR);
	cr0.remove(CR0_EMULATE_COPROCESSOR);

	// Prevent writes to read-only pages in Ring 0.
	cr0.insert(CR0_WRITE_PROTECT);

	// Enable caching.
	cr0.remove(CR0_CACHE_DISABLE | CR0_NOT_WRITE_THROUGH);

	unsafe { cr0_write(cr0); }

	//
	// CR4 CONFIGURATION
	//
	let mut cr4 = unsafe { cr4() };

	// Enable Machine Check Exceptions.
	// No need to check for support here, all x86-64 CPUs support it.
	cr4.insert(CR4_ENABLE_MACHINE_CHECK);

	// Enable full SSE support and indicates that the OS saves SSE context using FXSR.
	// No need to check for support here, all x86-64 CPUs support at least SSE2.
	cr4.insert(CR4_ENABLE_SSE | CR4_UNMASKED_SSE);

	if supports_xsave() {
		// Indicate that the OS saves extended context (AVX, AVX2, MPX, etc.) using XSAVE.
		cr4.insert(CR4_ENABLE_OS_XSAVE);

		// Enable saving the context for all known vector extensions.
		let mut xcr0 = unsafe { xcr0() };
		xcr0.insert(XCR0_FPU_MMX_STATE | XCR0_SSE_STATE);

		if supports_avx() {
			xcr0.insert(XCR0_AVX_STATE);
		}
	}

	// Enable FSGSBASE if available to read and write FS and GS faster.
	if supports_fsgsbase() {
		cr4.insert(CR4_ENABLE_FSGSBASE);

		// Use NOPs to patch out jumps over FSGSBASE usage in entry.asm.
		unsafe {
			ptr::write_bytes(&mut Lpatch0 as *mut u8, 0x90, 2);
			ptr::write_bytes(&mut Lpatch1 as *mut u8, 0x90, 2);
			ptr::write_bytes(&mut Lpatch2 as *mut u8, 0x90, 2);
		}
	}

	unsafe { cr4_write(cr4); }

	//
	// MSR CONFIGURATION
	//
	let mut efer = unsafe { rdmsr(IA32_EFER) };

	// Enable support for the EXECUTE_DISABLE paging bit.
	// No need to check for support here, it is always supported in x86-64 long mode.
	efer |= EFER_NXE;
	unsafe { wrmsr(IA32_EFER, efer); }

	// Initialize the FS register, which is later used for Thread-Local Storage.
	unsafe { writefs(0); }

	// Initialize the GS register, which is used for the per_core offset.
	unsafe {
		let size = &percore_end0 as *const u8 as usize - &percore_start as *const u8 as usize;
		let offset = current_boot_id as usize * size;
		writegs(offset);
		wrmsr(IA32_KERNEL_GS_BASE, 0);
	}

	// Initialize the core ID.
	unsafe { __core_id.set_per_core(current_boot_id as u32); }

	// TODO: Detect Enhanced Speed Step and enable maximum performance if possible.
	//unsafe { check_est(1); }
}

pub fn detect_frequency() {
	unsafe { CPU_FREQUENCY.detect(); }
}

pub fn print_information() {
	let cpuid = CpuId::new();
	let extended_function_info = cpuid.get_extended_function_info().expect("CPUID Extended Function Info not available!");
	let brand_string = extended_function_info.processor_brand_string().expect("CPUID Brand String not available!");
	let feature_printer = FeaturePrinter::new(&cpuid);

	info!("");
	info!("=============================== CPU INFORMATION ===============================");
	info!("Model:                  {}", brand_string);
	unsafe {
	info!("Frequency:              {}", CPU_FREQUENCY );
	}
	info!("Features:               {}", feature_printer);
	info!("Physical Address Width: {} bits", get_physical_address_bits());
	info!("Linear Address Width:   {} bits", get_linear_address_bits());
	info!("Supports 1GiB Pages:    {}", if supports_1gib_pages() { "Yes" } else { "No" });
	info!("===============================================================================");
	info!("");
}

#[inline]
pub fn get_linear_address_bits() -> u8 {
	unsafe { LINEAR_ADDRESS_BITS }
}

#[inline]
pub fn get_physical_address_bits() -> u8 {
	unsafe { PHYSICAL_ADDRESS_BITS }
}

#[inline]
pub fn supports_1gib_pages() -> bool {
	unsafe { SUPPORTS_1GIB_PAGES }
}

#[inline]
pub fn supports_avx() -> bool {
	unsafe { SUPPORTS_AVX }
}

#[inline]
pub fn supports_fsgsbase() -> bool {
	unsafe { SUPPORTS_FSGSBASE }
}

#[inline]
pub fn supports_xsave() -> bool {
	unsafe { SUPPORTS_XSAVE }
}

pub fn halt() {
	loop {
		unsafe {
			asm!("hlt" :::: "volatile");
		}
	}
}

#[inline(always)]
pub fn pause() {
	unsafe {
		asm!("pause" :::: "volatile");
	}
}


#[no_mangle]
pub extern "C" fn cpu_detection() -> i32 {
	configure();
	0
}

#[no_mangle]
pub extern "C" fn get_cpu_frequency() -> u32 {
	unsafe { CPU_FREQUENCY.get() as u32 }
}

#[no_mangle]
pub unsafe extern "C" fn fpu_init(fpu_state: *mut XSaveArea) {
	if supports_xsave() {
		ptr::write_bytes(fpu_state, 0, 1);
	} else {
		ptr::write_bytes(&mut (*fpu_state).legacy_region as *mut XSaveLegacyRegion, 0, 1);
	}

	(*fpu_state).legacy_region.fpu_control_word = 0x37f;
	(*fpu_state).legacy_region.mxcsr = 0x1f80;
}

#[no_mangle]
pub unsafe extern "C" fn restore_fpu_state(fpu_state: *const XSaveArea) {
	if supports_xsave() {
		let bitmask: u32 = !0;
		asm!("xrstorq $0" :: "*m"(fpu_state), "{eax}"(bitmask), "{edx}"(bitmask));
	} else {
		asm!("fxrstor $0" :: "*m"(fpu_state));
	}
}

#[no_mangle]
pub unsafe extern "C" fn save_fpu_state(fpu_state: *mut XSaveArea) {
	if supports_xsave() {
		let bitmask: u32 = !0;
		asm!("xsaveq $0" : "=*m"(fpu_state) : "{eax}"(bitmask), "{edx}"(bitmask) : "memory");
	} else {
		asm!("fxsave $0; fnclex" : "=*m"(fpu_state) :: "memory");
	}
}

#[no_mangle]
pub unsafe extern "C" fn readfs() -> usize {
	if supports_fsgsbase() {
		let fs: usize;
		asm!("rdfsbase $0" : "=r"(fs) :: "memory");
		fs
	} else {
		rdmsr(IA32_FS_BASE) as usize
	}
}

#[no_mangle]
pub unsafe extern "C" fn writefs(fs: usize) {
	if supports_fsgsbase() {
		asm!("wrfsbase $0" :: "r"(fs));
	} else {
		wrmsr(IA32_FS_BASE, fs as u64);
	}
}

#[no_mangle]
pub unsafe extern "C" fn writegs(gs: usize) {
	if supports_fsgsbase() {
		asm!("wrgsbase $0" :: "r"(gs));
	} else {
		wrmsr(IA32_GS_BASE, gs as u64);
	}
}

#[inline]
unsafe fn get_timestamp_rdtsc() -> u64 {
	asm!("lfence" ::: "memory");
	let value = rdtsc();
	asm!("lfence" ::: "memory");
	value
}

#[inline]
unsafe fn get_timestamp_rdtscp() -> u64 {
	let value = rdtscp();
	asm!("lfence" ::: "memory");
	value
}

#[no_mangle]
pub unsafe extern "C" fn udelay(usecs: u32) {
	let deadline = get_cpu_frequency() as u64 * usecs as u64;
	let start = TIMESTAMP_FUNCTION();

	loop {
		let end = TIMESTAMP_FUNCTION();

		let diff = if end > start { end - start } else { start - end };
		if diff >= deadline {
			break;
		}

		if deadline - diff > 50000 {
			check_workqueues_in_irqhandler(-1);
		}
	}
}
