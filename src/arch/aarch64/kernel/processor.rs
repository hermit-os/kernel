use core::arch::asm;
use core::{fmt, str};

use aarch64::regs::*;
use hermit_dtb::Dtb;
use hermit_sync::{Lazy, OnceCell, without_interrupts};

use crate::env;

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
				const PSCI_SYSTEM_OFF: u64 = 0x8400_0008;
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
	let dtb = unsafe {
		Dtb::from_raw(core::ptr::with_exposed_provenance(
			env::boot_info().hardware_info.device_tree.unwrap().get() as usize,
		))
		.expect(".dtb file has invalid header")
	};

	let reg = dtb
		.get_property("/cpus/cpu@0", "compatible")
		.unwrap_or(b"unknown");

	infoheader!(" CPU INFORMATION ");
	infoentry!("Processor compatibility", str::from_utf8(reg).unwrap());
	infoentry!("Counter frequency", *CPU_FREQUENCY);
	infofooter!();
}
