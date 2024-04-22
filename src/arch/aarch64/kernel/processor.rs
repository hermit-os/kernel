use core::arch::asm;
use core::{fmt, str};

use aarch64::regs::{Readable, CNTFRQ_EL0};
use hermit_dtb::Dtb;
use hermit_sync::{without_interrupts, Lazy};

use crate::arch::aarch64::kernel::boot_info;
use crate::env;

// System counter frequency in Hz
static CPU_FREQUENCY: Lazy<CpuFrequency> = Lazy::new(|| {
	let mut cpu_frequency = CpuFrequency::new();
	unsafe {
		cpu_frequency.detect();
	}
	cpu_frequency
});

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
			_ => panic!("Attempted to print an invalid CPU Frequency Source"),
		}
	}
}

struct CpuFrequency {
	hz: u32,
	source: CpuFrequencySources,
}

impl CpuFrequency {
	const fn new() -> Self {
		CpuFrequency {
			hz: 0,
			source: CpuFrequencySources::Invalid,
		}
	}

	fn set_detected_cpu_frequency(
		&mut self,
		hz: u32,
		source: CpuFrequencySources,
	) -> Result<(), ()> {
		//The clock frequency must never be set to zero, otherwise a division by zero will
		//occur during runtime
		if hz > 0 {
			self.hz = hz;
			self.source = source;
			Ok(())
		} else {
			Err(())
		}
	}

	unsafe fn detect_from_cmdline(&mut self) -> Result<(), ()> {
		let mhz = env::freq().ok_or(())?;
		self.set_detected_cpu_frequency(u32::from(mhz) * 1000000, CpuFrequencySources::CommandLine)
	}

	unsafe fn detect_from_register(&mut self) -> Result<(), ()> {
		let hz = CNTFRQ_EL0.get() & 0xFFFFFFFF;
		self.set_detected_cpu_frequency(hz.try_into().unwrap(), CpuFrequencySources::Register)
	}

	unsafe fn detect(&mut self) {
		unsafe {
			self.detect_from_register()
				.or_else(|_e| self.detect_from_cmdline())
				.unwrap();
		}
	}

	fn get(&self) -> u32 {
		self.hz
	}
}

impl fmt::Display for CpuFrequency {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "{} Hz (from {})", self.hz, self.source)
	}
}

pub fn seed_entropy() -> Option<[u8; 32]> {
	None
}

pub(crate) fn run_on_hypervisor() -> bool {
	true
}

/// The halt function stops the processor until the next interrupt arrives
pub fn halt() {
	unsafe {
		asm!("wfi", options(nostack, nomem),);
	}
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
				const PSCI_SYSTEM_OFF: u64 = 0x84000008;
				// call hypervisor to shut down the system
				asm!("hvc #0", in("x0") PSCI_SYSTEM_OFF, options(nomem, nostack));

				// we should never reach this point
				loop {
					asm!("wfe", options(nomem, nostack));
				}
			}
		}
	}
}

#[inline]
pub fn get_timer_ticks() -> u64 {
	// We simulate a timer with a 1 microsecond resolution by taking the CPU timestamp
	// and dividing it by the CPU frequency in MHz.
	let ticks = 1000000 * u128::from(get_timestamp()) / u128::from(CPU_FREQUENCY.get());
	u64::try_from(ticks).unwrap()
}

#[inline]
pub fn get_frequency() -> u16 {
	(CPU_FREQUENCY.get() / 1000000).try_into().unwrap()
}

#[inline]
pub fn get_timestamp() -> u64 {
	let value: u64;

	unsafe {
		asm!(
			"mrs {value}, cntpct_el0",
			value = out(reg) value,
			options(nostack),
		);
	}

	value
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
		let pmuserenr_el0: u64 = 1 << 0 | 1 << 2 | 1 << 3;
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
		debug!(
			"PMCR_EL0 (has RES1 bits and therefore mustn't be zero): {:#X}",
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

pub fn detect_frequency() {
	Lazy::force(&CPU_FREQUENCY);
}

#[inline]
fn __set_oneshot_timer(wakeup_time: Option<u64>) {
	if let Some(wt) = wakeup_time {
		// wt is the absolute wakeup time in microseconds based on processor::get_timer_ticks.
		let deadline = u128::from(wt) * u128::from(CPU_FREQUENCY.get()) / 1000000;
		let deadline = u64::try_from(deadline).unwrap();

		unsafe {
			asm!(
				"msr cntp_cval_el0, {value}",
				"msr cntp_ctl_el0, {enable}",
				value = in(reg) deadline,
				enable = in(reg) 1u64,
				options(nostack, nomem),
			);
		}
	} else {
		// disable timer
		unsafe {
			asm!(
				"msr cntp_cval_el0, xzr",
				"msr cntp_ctl_el0, xzr",
				options(nostack, nomem),
			);
		}
	}
}

pub fn set_oneshot_timer(wakeup_time: Option<u64>) {
	without_interrupts(|| {
		__set_oneshot_timer(wakeup_time);
	});
}

pub fn print_information() {
	let dtb = unsafe {
		Dtb::from_raw(core::ptr::with_exposed_provenance(
			boot_info().hardware_info.device_tree.unwrap().get() as usize,
		))
		.expect(".dtb file has invalid header")
	};

	let reg = dtb
		.get_property("/cpus/cpu@0", "compatible")
		.unwrap_or(b"unknown");

	infoheader!(" CPU INFORMATION ");
	infoentry!("Processor compatiblity", str::from_utf8(reg).unwrap());
	infoentry!("Counter frequency", *CPU_FREQUENCY);
	if run_on_hypervisor() {
		info!("Run on hypervisor");
	}
	infofooter!();
}
