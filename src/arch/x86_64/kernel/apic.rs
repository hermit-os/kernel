use crate::arch;
#[cfg(feature = "acpi")]
use crate::arch::x86_64::kernel::acpi;
use crate::arch::x86_64::kernel::irq::IrqStatistics;
use crate::arch::x86_64::kernel::{CURRENT_STACK_ADDRESS, IRQ_COUNTERS};
use crate::arch::x86_64::mm::paging::{
	BasePageSize, PageSize, PageTableEntryFlags, PageTableEntryFlagsExt,
};
use crate::arch::x86_64::mm::{paging, virtualmem};
use crate::arch::x86_64::mm::{PhysAddr, VirtAddr};
use crate::collections::irqsave;
use crate::config::*;
use crate::env;
use crate::mm;
use crate::scheduler;
use crate::scheduler::CoreId;
#[cfg(feature = "smp")]
use crate::x86::controlregs::*;
use crate::x86::msr::*;
use alloc::boxed::Box;
use alloc::vec::Vec;
use arch::x86_64::kernel::{idt, irq, percore::*, processor};
#[cfg(any(feature = "pci", feature = "smp"))]
use core::arch::x86_64::_mm_mfence;
use core::hint::spin_loop;
#[cfg(feature = "smp")]
use core::ptr;
use core::sync::atomic::Ordering;
use core::{cmp, fmt, mem, u32};
use crossbeam_utils::CachePadded;

const MP_FLT_SIGNATURE: u32 = 0x5f504d5f;
const MP_CONFIG_SIGNATURE: u32 = 0x504d4350;

const APIC_ICR2: usize = 0x0310;

const APIC_DIV_CONF_DIVIDE_BY_8: u64 = 0b0010;
const APIC_EOI_ACK: u64 = 0;
#[cfg(feature = "smp")]
const APIC_ICR_DELIVERY_MODE_FIXED: u64 = 0x000;
#[cfg(feature = "smp")]
const APIC_ICR_DELIVERY_MODE_INIT: u64 = 0x500;
#[cfg(feature = "smp")]
const APIC_ICR_DELIVERY_MODE_STARTUP: u64 = 0x600;
const APIC_ICR_DELIVERY_STATUS_PENDING: u32 = 1 << 12;
#[cfg(feature = "smp")]
const APIC_ICR_LEVEL_TRIGGERED: u64 = 1 << 15;
#[cfg(feature = "smp")]
const APIC_ICR_LEVEL_ASSERT: u64 = 1 << 14;
const APIC_LVT_MASK: u64 = 1 << 16;
const APIC_LVT_TIMER_TSC_DEADLINE: u64 = 1 << 18;
const APIC_SIVR_ENABLED: u64 = 1 << 8;

/// Register index: ID
#[allow(dead_code)]
const IOAPIC_REG_ID: u32 = 0x0000;
/// Register index: version
const IOAPIC_REG_VER: u32 = 0x0001;
/// Redirection table base
const IOAPIC_REG_TABLE: u32 = 0x0010;

#[cfg(feature = "smp")]
const TLB_FLUSH_INTERRUPT_NUMBER: u8 = 112;
#[cfg(feature = "smp")]
const WAKEUP_INTERRUPT_NUMBER: u8 = 121;
pub const TIMER_INTERRUPT_NUMBER: u8 = 123;
const ERROR_INTERRUPT_NUMBER: u8 = 126;
const SPURIOUS_INTERRUPT_NUMBER: u8 = 127;

/// Physical and virtual memory address for our SMP boot code.
///
/// While our boot processor is already in x86-64 mode, application processors boot up in 16-bit real mode
/// and need an address in the CS:IP addressing scheme to jump to.
/// The CS:IP addressing scheme is limited to 2^20 bytes (= 1 MiB).
#[cfg(feature = "smp")]
const SMP_BOOT_CODE_ADDRESS: VirtAddr = VirtAddr(0x8000);

#[cfg(feature = "smp")]
const SMP_BOOT_CODE_OFFSET_ENTRY: usize = 0x08;
#[cfg(feature = "smp")]
const SMP_BOOT_CODE_OFFSET_CPU_ID: usize = SMP_BOOT_CODE_OFFSET_ENTRY + 0x08;
#[cfg(feature = "smp")]
const SMP_BOOT_CODE_OFFSET_BOOTINFO: usize = SMP_BOOT_CODE_OFFSET_CPU_ID + 0x04;
#[cfg(feature = "smp")]
const SMP_BOOT_CODE_OFFSET_PML4: usize = SMP_BOOT_CODE_OFFSET_BOOTINFO + 0x08;

const X2APIC_ENABLE: u64 = 1 << 10;

static mut LOCAL_APIC_ADDRESS: VirtAddr = VirtAddr::zero();
static mut IOAPIC_ADDRESS: VirtAddr = VirtAddr::zero();

/// Stores the Local APIC IDs of all CPUs. The index equals the Core ID.
/// Both numbers often match, but don't need to (e.g. when a core has been disabled).
///
/// As Rust currently implements no way of zero-initializing a global Vec in a no_std environment,
/// we have to encapsulate it in an Option...
static mut CPU_LOCAL_APIC_IDS: Option<Vec<u8>> = None;

/// After calibration, initialize the APIC Timer with this counter value to let it fire an interrupt
/// after 1 microsecond.
static mut CALIBRATED_COUNTER_VALUE: u64 = 0;

/// MP Floating Pointer Structure
#[repr(C, packed)]
struct ApicMP {
	signature: u32,
	mp_config: u32,
	length: u8,
	version: u8,
	checksum: u8,
	features: [u8; 5],
}

/// MP Configuration Table
#[repr(C, packed)]
struct ApicConfigTable {
	signature: u32,
	length: u16,
	revision: u8,
	checksum: u8,
	oem_id: [u8; 8],
	product_id: [u8; 12],
	oem_table: u32,
	oem_table_size: u16,
	entry_count: u16,
	lapic: u32,
	extended_table_length: u16,
	extended_table_checksum: u8,
	reserved: u8,
}

/// APIC Processor Entry
#[repr(C, packed)]
struct ApicProcessorEntry {
	typ: u8,
	id: u8,
	version: u8,
	cpu_flags: u8,
	cpu_signature: u32,
	cpu_feature: u32,
	reserved: [u32; 2],
}

/// IO APIC Entry
#[repr(C, packed)]
struct ApicIoEntry {
	typ: u8,
	id: u8,
	version: u8,
	enabled: u8,
	addr: u32,
}

#[cfg(feature = "acpi")]
#[repr(C, packed)]
struct AcpiMadtHeader {
	local_apic_address: u32,
	flags: u32,
}

#[cfg(feature = "acpi")]
#[repr(C, packed)]
struct AcpiMadtRecordHeader {
	entry_type: u8,
	length: u8,
}

#[repr(C, packed)]
struct ProcessorLocalApicRecord {
	acpi_processor_id: u8,
	apic_id: u8,
	flags: u32,
}

impl fmt::Display for ProcessorLocalApicRecord {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "{{ acpi_processor_id: {}, ", { self.acpi_processor_id })?;
		write!(f, "apic_id: {}, ", { self.apic_id })?;
		write!(f, "flags: {} }}", { self.flags })?;
		Ok(())
	}
}

#[cfg(feature = "acpi")]
const CPU_FLAG_ENABLED: u32 = 1 << 0;

#[repr(C, packed)]
struct IoApicRecord {
	id: u8,
	reserved: u8,
	address: u32,
	global_system_interrupt_base: u32,
}

impl fmt::Display for IoApicRecord {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "{{ id: {}, ", { self.id })?;
		write!(f, "reserved: {}, ", { self.reserved })?;
		write!(f, "address: {:#X}, ", { self.address })?;
		write!(f, "global_system_interrupt_base: {} }}", {
			self.global_system_interrupt_base
		})?;
		Ok(())
	}
}

#[cfg(feature = "smp")]
extern "x86-interrupt" fn tlb_flush_handler(_stack_frame: irq::ExceptionStackFrame) {
	debug!("Received TLB Flush Interrupt");
	increment_irq_counter(TLB_FLUSH_INTERRUPT_NUMBER.into());
	unsafe {
		cr3_write(cr3());
	}
	eoi();
}

extern "x86-interrupt" fn error_interrupt_handler(stack_frame: irq::ExceptionStackFrame) {
	error!("APIC LVT Error Interrupt");
	error!("ESR: {:#X}", local_apic_read(IA32_X2APIC_ESR));
	error!("{:#?}", stack_frame);
	eoi();
	scheduler::abort();
}

extern "x86-interrupt" fn spurious_interrupt_handler(stack_frame: irq::ExceptionStackFrame) {
	error!("Spurious Interrupt: {:#?}", stack_frame);
	scheduler::abort();
}

#[cfg(feature = "smp")]
extern "x86-interrupt" fn wakeup_handler(_stack_frame: irq::ExceptionStackFrame) {
	debug!("Received Wakeup Interrupt");
	increment_irq_counter(WAKEUP_INTERRUPT_NUMBER.into());
	let core_scheduler = core_scheduler();
	core_scheduler.check_input();
	eoi();
	if core_scheduler.is_scheduling() {
		core_scheduler.scheduler();
	}
}

#[inline]
pub fn add_local_apic_id(id: u8) {
	unsafe {
		CPU_LOCAL_APIC_IDS.as_mut().unwrap().push(id);
	}
}

#[cfg(feature = "smp")]
pub fn local_apic_id_count() -> u32 {
	unsafe { CPU_LOCAL_APIC_IDS.as_ref().unwrap().len() as u32 }
}

#[cfg(not(feature = "acpi"))]
fn detect_from_acpi() -> Result<PhysAddr, ()> {
	// dummy implementation if acpi support is disabled
	Err(())
}

#[cfg(feature = "acpi")]
fn detect_from_acpi() -> Result<PhysAddr, ()> {
	// Get the Multiple APIC Description Table (MADT) from the ACPI information and its specific table header.
	let madt = acpi::get_madt().expect("HermitCore requires a MADT in the ACPI tables");
	let madt_header = unsafe { &*(madt.table_start_address() as *const AcpiMadtHeader) };

	// Jump to the actual table entries (after the table header).
	let mut current_address = madt.table_start_address() + mem::size_of::<AcpiMadtHeader>();

	// Loop through all table entries.
	while current_address < madt.table_end_address() {
		let record = unsafe { &*(current_address as *const AcpiMadtRecordHeader) };
		current_address += mem::size_of::<AcpiMadtRecordHeader>();

		match record.entry_type {
			0 => {
				// Processor Local APIC
				let processor_local_apic_record =
					unsafe { &*(current_address as *const ProcessorLocalApicRecord) };
				debug!(
					"Found Processor Local APIC record: {}",
					processor_local_apic_record
				);

				if processor_local_apic_record.flags & CPU_FLAG_ENABLED > 0 {
					add_local_apic_id(processor_local_apic_record.apic_id);
				}
			}
			1 => {
				// I/O APIC
				let ioapic_record = unsafe { &*(current_address as *const IoApicRecord) };
				debug!("Found I/O APIC record: {}", ioapic_record);

				unsafe {
					IOAPIC_ADDRESS = virtualmem::allocate(BasePageSize::SIZE as usize).unwrap();
					let record_addr = ioapic_record.address;
					debug!(
						"Mapping IOAPIC at {:#X} to virtual address {:#X}",
						record_addr, IOAPIC_ADDRESS
					);

					let mut flags = PageTableEntryFlags::empty();
					flags.device().writable().execute_disable();
					paging::map::<BasePageSize>(
						IOAPIC_ADDRESS,
						PhysAddr(record_addr.into()),
						1,
						flags,
					);
				}
			}
			_ => {
				// Just ignore other entries for now.
			}
		}

		current_address += record.length as usize - mem::size_of::<AcpiMadtRecordHeader>();
	}

	// Successfully derived all information from the MADT.
	// Return the physical address of the Local APIC.
	Ok(PhysAddr(madt_header.local_apic_address.into()))
}

/// Helper function to search Floating Pointer Structure of the Multiprocessing Specification
fn search_mp_floating(start: PhysAddr, end: PhysAddr) -> Result<&'static ApicMP, ()> {
	let virtual_address = virtualmem::allocate(BasePageSize::SIZE as usize).map_err(|_| ())?;

	for current_address in (start.as_usize()..end.as_usize()).step_by(BasePageSize::SIZE as usize) {
		let mut flags = PageTableEntryFlags::empty();
		flags.normal().writable();
		paging::map::<BasePageSize>(
			virtual_address,
			PhysAddr::from(align_down!(current_address, BasePageSize::SIZE as usize)),
			1,
			flags,
		);

		for i in 0..BasePageSize::SIZE / 4 {
			let mut tmp: *const u32 = virtual_address.as_ptr();
			tmp = unsafe { tmp.offset(i.try_into().unwrap()) };
			let apic_mp: &ApicMP = unsafe { &(*(tmp as *const ApicMP)) };
			if apic_mp.signature == MP_FLT_SIGNATURE
				&& !(apic_mp.version > 4 || apic_mp.features[0] != 0)
			{
				return Ok(apic_mp);
			}
		}
	}

	// frees obsolete virtual memory region for MMIO devices
	virtualmem::deallocate(virtual_address, BasePageSize::SIZE as usize);

	Err(())
}

/// Helper function to detect APIC by the Multiprocessor Specification
fn detect_from_mp() -> Result<PhysAddr, ()> {
	let mp_float = if let Ok(mpf) = search_mp_floating(PhysAddr(0x9F000u64), PhysAddr(0xA0000u64)) {
		Ok(mpf)
	} else if let Ok(mpf) = search_mp_floating(PhysAddr(0xF0000u64), PhysAddr(0x100000u64)) {
		Ok(mpf)
	} else {
		Err(())
	}?;

	info!("Found MP config at {:#x}", { mp_float.mp_config });
	info!(
		"System uses Multiprocessing Specification 1.{}",
		mp_float.version
	);
	info!("MP features 1: {}", mp_float.features[0]);

	if mp_float.features[1] & 0x80 > 0 {
		info!("PIC mode implemented");
	} else {
		info!("Virtual-Wire mode implemented");
	}

	let virtual_address = virtualmem::allocate(BasePageSize::SIZE as usize).map_err(|_| ())?;

	let mut flags = PageTableEntryFlags::empty();
	flags.normal().writable();
	paging::map::<BasePageSize>(
		virtual_address,
		PhysAddr::from(align_down!(
			mp_float.mp_config as usize,
			BasePageSize::SIZE as usize
		)),
		1,
		flags,
	);

	let mut addr: usize = virtual_address.as_usize()
		| (mp_float.mp_config as usize & (BasePageSize::SIZE as usize - 1));
	let mp_config: &ApicConfigTable = unsafe { &*(addr as *const ApicConfigTable) };
	if mp_config.signature != MP_CONFIG_SIGNATURE {
		warn!("Invalid MP config table");
		virtualmem::deallocate(virtual_address, BasePageSize::SIZE as usize);
		return Err(());
	}

	if mp_config.entry_count == 0 {
		warn!("No MP table entries! Guess IO-APIC!");
		let default_address = PhysAddr(0xFEC0_0000);

		unsafe {
			IOAPIC_ADDRESS = virtualmem::allocate(BasePageSize::SIZE as usize).unwrap();
			debug!(
				"Mapping IOAPIC at {:#X} to virtual address {:#X}",
				default_address, IOAPIC_ADDRESS
			);

			let mut flags = PageTableEntryFlags::empty();
			flags.device().writable().execute_disable();
			paging::map::<BasePageSize>(IOAPIC_ADDRESS, default_address, 1, flags);
		}
	} else {
		// entries starts directly after the config table
		addr += mem::size_of::<ApicConfigTable>();
		for _i in 0..mp_config.entry_count {
			match unsafe { *(addr as *const u8) } {
				// CPU entry
				0 => {
					let cpu_entry: &ApicProcessorEntry =
						unsafe { &*(addr as *const ApicProcessorEntry) };
					if cpu_entry.cpu_flags & 0x01 == 0x01 {
						add_local_apic_id(cpu_entry.id);
					}
					addr += mem::size_of::<ApicProcessorEntry>();
				}
				// IO-APIC entry
				2 => {
					let io_entry: &ApicIoEntry = unsafe { &*(addr as *const ApicIoEntry) };
					let ioapic = io_entry.addr;
					info!("Found IOAPIC at 0x{:x}", ioapic);

					unsafe {
						IOAPIC_ADDRESS = virtualmem::allocate(BasePageSize::SIZE as usize).unwrap();
						debug!(
							"Mapping IOAPIC at {:#X} to virtual address {:#X}",
							ioapic, IOAPIC_ADDRESS
						);

						let mut flags = PageTableEntryFlags::empty();
						flags.device().writable().execute_disable();
						paging::map::<BasePageSize>(
							IOAPIC_ADDRESS,
							PhysAddr(ioapic as u64),
							1,
							flags,
						);
					}

					addr += mem::size_of::<ApicIoEntry>();
				}
				_ => {
					addr += 8;
				}
			}
		}
	}

	Ok(PhysAddr(mp_config.lapic as u64))
}

fn default_apic() -> PhysAddr {
	warn!("Try to use default APIC address");

	let default_address = PhysAddr(0xFEC0_0000);

	unsafe {
		IOAPIC_ADDRESS = virtualmem::allocate(BasePageSize::SIZE as usize).unwrap();
		debug!(
			"Mapping IOAPIC at {:#X} to virtual address {:#X}",
			default_address, IOAPIC_ADDRESS
		);

		let mut flags = PageTableEntryFlags::empty();
		flags.device().writable().execute_disable();
		paging::map::<BasePageSize>(IOAPIC_ADDRESS, default_address, 1, flags);
	}

	PhysAddr(0xFEE0_0000)
}

fn detect_from_uhyve() -> Result<PhysAddr, ()> {
	if env::is_uhyve() {
		let default_address = PhysAddr(0xFEC0_0000);

		unsafe {
			IOAPIC_ADDRESS = virtualmem::allocate(BasePageSize::SIZE as usize).unwrap();
			debug!(
				"Mapping IOAPIC at {:#X} to virtual address {:#X}",
				default_address, IOAPIC_ADDRESS
			);

			let mut flags = PageTableEntryFlags::empty();
			flags.device().writable().execute_disable();
			paging::map::<BasePageSize>(IOAPIC_ADDRESS, default_address, 1, flags);
		}

		return Ok(PhysAddr(0xFEE0_0000));
	}

	Err(())
}

#[no_mangle]
pub extern "C" fn eoi() {
	local_apic_write(IA32_X2APIC_EOI, APIC_EOI_ACK);
}

pub fn init() {
	let boxed_irq = Box::new(IrqStatistics::new());
	let boxed_irq_raw = Box::into_raw(boxed_irq);
	unsafe {
		IRQ_COUNTERS.insert(0, &(*boxed_irq_raw));
		PERCORE.irq_statistics.set(boxed_irq_raw);
	}

	// Initialize an empty vector for the Local APIC IDs of all CPUs.
	unsafe {
		CPU_LOCAL_APIC_IDS = Some(Vec::new());
	}

	// Detect CPUs and APICs.
	let local_apic_physical_address = detect_from_uhyve()
		.or_else(|_| detect_from_acpi())
		.or_else(|_| detect_from_mp())
		.unwrap_or_else(|_| default_apic());

	// Initialize x2APIC or xAPIC, depending on what's available.
	init_x2apic();
	if !processor::supports_x2apic() {
		// We use the traditional xAPIC mode available on all x86-64 CPUs.
		// It uses a mapped page for communication.
		unsafe {
			LOCAL_APIC_ADDRESS = virtualmem::allocate(BasePageSize::SIZE as usize).unwrap();
			debug!(
				"Mapping Local APIC at {:#X} to virtual address {:#X}",
				local_apic_physical_address, LOCAL_APIC_ADDRESS
			);

			let mut flags = PageTableEntryFlags::empty();
			flags.device().writable().execute_disable();
			paging::map::<BasePageSize>(LOCAL_APIC_ADDRESS, local_apic_physical_address, 1, flags);
		}
	}

	// Set gates to ISRs for the APIC interrupts we are going to enable.
	#[cfg(feature = "smp")]
	idt::set_gate(TLB_FLUSH_INTERRUPT_NUMBER, tlb_flush_handler as usize, 0);
	#[cfg(feature = "smp")]
	irq::add_irq_name((TLB_FLUSH_INTERRUPT_NUMBER - 32).into(), "TLB flush");
	idt::set_gate(ERROR_INTERRUPT_NUMBER, error_interrupt_handler as usize, 0);
	idt::set_gate(
		SPURIOUS_INTERRUPT_NUMBER,
		spurious_interrupt_handler as usize,
		0,
	);
	#[cfg(feature = "smp")]
	idt::set_gate(WAKEUP_INTERRUPT_NUMBER, wakeup_handler as usize, 0);
	#[cfg(feature = "smp")]
	irq::add_irq_name((WAKEUP_INTERRUPT_NUMBER - 32).into(), "Wakeup");

	// Initialize interrupt handling over APIC.
	// All interrupts of the PIC have already been masked, so it doesn't need to be disabled again.
	init_local_apic();

	if !processor::supports_tsc_deadline() {
		// We have an older APIC Timer without TSC Deadline support, which has a maximum timeout
		// and needs to be calibrated.
		calibrate_timer();
	}

	// init ioapic
	init_ioapic();
}

fn init_ioapic() {
	let max_entry = ioapic_max_redirection_entry() + 1;
	info!("IOAPIC v{} has {} entries", ioapic_version(), max_entry);

	// now lets turn everything else on
	for i in 0..max_entry {
		if i != 2 {
			ioapic_inton(i, 0 /*apic_processors[boot_processor]->id*/).unwrap();
		} else {
			// now, we don't longer need the IOAPIC timer and turn it off
			info!("Disable IOAPIC timer");
			ioapic_intoff(2, 0 /*apic_processors[boot_processor]->id*/).unwrap();
		}
	}
}

fn ioapic_inton(irq: u8, apicid: u8) -> Result<(), ()> {
	if irq > 24 {
		error!("IOAPIC: trying to turn on irq {} which is too high\n", irq);
		return Err(());
	}

	let off = u32::from(irq * 2);
	let ioredirect_upper: u32 = u32::from(apicid) << 24;
	let ioredirect_lower: u32 = u32::from(0x20 + irq);

	ioapic_write(IOAPIC_REG_TABLE + off, ioredirect_lower);
	ioapic_write(IOAPIC_REG_TABLE + 1 + off, ioredirect_upper);

	Ok(())
}

fn ioapic_intoff(irq: u32, apicid: u32) -> Result<(), ()> {
	if irq > 24 {
		error!("IOAPIC: trying to turn off irq {} which is too high\n", irq);
		return Err(());
	}

	let off = irq * 2;
	let ioredirect_upper: u32 = apicid << 24;
	let ioredirect_lower: u32 = (0x20 + irq) | (1 << 16); // turn it off (start masking)

	ioapic_write(IOAPIC_REG_TABLE + off, ioredirect_lower);
	ioapic_write(IOAPIC_REG_TABLE + 1 + off, ioredirect_upper);

	Ok(())
}

pub fn init_local_apic() {
	// Mask out all interrupts we don't need right now.
	local_apic_write(IA32_X2APIC_LVT_TIMER, APIC_LVT_MASK);
	local_apic_write(IA32_X2APIC_LVT_THERMAL, APIC_LVT_MASK);
	local_apic_write(IA32_X2APIC_LVT_PMI, APIC_LVT_MASK);
	local_apic_write(IA32_X2APIC_LVT_LINT0, APIC_LVT_MASK);
	local_apic_write(IA32_X2APIC_LVT_LINT1, APIC_LVT_MASK);

	// Set the interrupt number of the Error interrupt.
	local_apic_write(IA32_X2APIC_LVT_ERROR, u64::from(ERROR_INTERRUPT_NUMBER));

	// allow all interrupts
	local_apic_write(IA32_X2APIC_TPR, 0x00);

	// Finally, enable the Local APIC by setting the interrupt number for spurious interrupts
	// and providing the enable bit.
	local_apic_write(
		IA32_X2APIC_SIVR,
		APIC_SIVR_ENABLED | (u64::from(SPURIOUS_INTERRUPT_NUMBER)),
	);
}

fn calibrate_timer() {
	// The APIC Timer is used to provide a one-shot interrupt for the tickless timer
	// implemented through processor::get_timer_ticks.
	// Therefore determine a counter value for 1 microsecond, which is the resolution
	// used throughout all of HermitCore. Wait 30ms for accuracy.
	let microseconds = 30_000;

	// Be sure that all interrupts for calibration accuracy and initialize the counter are disabled.
	// Dividing the counter value by 8 still provides enough accuracy for 1 microsecond resolution,
	// but allows for longer timeouts than a smaller divisor.
	// For example, on an Intel Xeon E5-2650 v3 @ 2.30GHz, the counter is usually calibrated to
	// 125, which allows for timeouts of approximately 34 seconds (u32::MAX / 125).

	local_apic_write(IA32_X2APIC_DIV_CONF, APIC_DIV_CONF_DIVIDE_BY_8);
	local_apic_write(IA32_X2APIC_INIT_COUNT, u64::from(u32::MAX));

	// Wait until the calibration time has elapsed.
	processor::udelay(microseconds);

	// Save the difference of the initial value and current value as the result of the calibration
	// and re-enable interrupts.
	unsafe {
		CALIBRATED_COUNTER_VALUE =
			(u64::from(u32::MAX - local_apic_read(IA32_X2APIC_CUR_COUNT))) / microseconds;
		debug!(
			"Calibrated APIC Timer with a counter value of {} for 1 microsecond",
			CALIBRATED_COUNTER_VALUE
		);
	}
}

fn __set_oneshot_timer(wakeup_time: Option<u64>) {
	if let Some(wt) = wakeup_time {
		if processor::supports_tsc_deadline() {
			// wt is the absolute wakeup time in microseconds based on processor::get_timer_ticks.
			// We can simply multiply it by the processor frequency to get the absolute Time-Stamp Counter deadline
			// (see processor::get_timer_ticks).
			let tsc_deadline = wt * (u64::from(processor::get_frequency()));

			// Enable the APIC Timer in TSC-Deadline Mode and let it start by writing to the respective MSR.
			local_apic_write(
				IA32_X2APIC_LVT_TIMER,
				APIC_LVT_TIMER_TSC_DEADLINE | u64::from(TIMER_INTERRUPT_NUMBER),
			);
			unsafe {
				wrmsr(IA32_TSC_DEADLINE, tsc_deadline);
			}
		} else {
			// Calculate the relative timeout from the absolute wakeup time.
			// Maintain a minimum value of one tick, otherwise the timer interrupt does not fire at all.
			// The Timer Counter Register is also a 32-bit register, which we must not overflow for longer timeouts.
			let current_time = processor::get_timer_ticks();
			let ticks = if wt > current_time {
				wt - current_time
			} else {
				1
			};
			let init_count = cmp::min(
				unsafe { CALIBRATED_COUNTER_VALUE } * ticks,
				u64::from(u32::MAX),
			);

			// Enable the APIC Timer in One-Shot Mode and let it start by setting the initial counter value.
			local_apic_write(IA32_X2APIC_LVT_TIMER, u64::from(TIMER_INTERRUPT_NUMBER));
			local_apic_write(IA32_X2APIC_INIT_COUNT, init_count);
		}
	} else {
		// Disable the APIC Timer.
		local_apic_write(IA32_X2APIC_LVT_TIMER, APIC_LVT_MASK);
	}
}

pub fn set_oneshot_timer(wakeup_time: Option<u64>) {
	irqsave(|| {
		__set_oneshot_timer(wakeup_time);
	});
}

pub fn init_x2apic() {
	if processor::supports_x2apic() {
		debug!("Enable x2APIC support");
		// The CPU supports the modern x2APIC mode, which uses MSRs for communication.
		// Enable it.
		let mut apic_base = unsafe { rdmsr(IA32_APIC_BASE) };
		apic_base |= X2APIC_ENABLE;
		unsafe {
			wrmsr(IA32_APIC_BASE, apic_base);
		}
	}
}

/// Initialize the required _start variables for the next CPU to be booted.
pub fn init_next_processor_variables(core_id: CoreId) {
	// Allocate stack and PerCoreVariables structure for the CPU and pass the addresses.
	// Keep the stack executable to possibly support dynamically generated code on the stack (see https://security.stackexchange.com/a/47825).
	let stack = mm::allocate(KERNEL_STACK_SIZE, true);
	let mut boxed_percore = Box::new(CachePadded::new(PerCoreInnerVariables::new(core_id)));
	let boxed_irq = Box::new(IrqStatistics::new());
	let boxed_irq_raw = Box::into_raw(boxed_irq);

	unsafe {
		IRQ_COUNTERS.insert(core_id, &(*boxed_irq_raw));
		boxed_percore.irq_statistics = PerCoreVariable::new(boxed_irq_raw);
	}

	CURRENT_STACK_ADDRESS.store(stack.as_u64(), Ordering::Relaxed);

	let current_percore = Box::leak(boxed_percore);

	trace!(
		"Initialize per core data at {:p} (size {} bytes)",
		current_percore,
		mem::size_of_val(current_percore)
	);

	CURRENT_PERCORE_ADDRESS.store(current_percore as *mut _ as u64, Ordering::Release);
}

/// Boot all Application Processors
/// This algorithm is derived from Intel MultiProcessor Specification 1.4, B.4, but testing has shown
/// that a second STARTUP IPI and setting the BIOS Reset Vector are no longer necessary.
/// This is partly confirmed by <https://wiki.osdev.org/Symmetric_Multiprocessing>
#[cfg(all(target_os = "none", feature = "smp"))]
pub fn boot_application_processors() {
	use core::hint;

	use include_transformed::include_nasm_bin;

	use super::{raw_boot_info, start};

	let smp_boot_code = include_nasm_bin!("boot.asm");

	// We shouldn't have any problems fitting the boot code into a single page, but let's better be sure.
	assert!(
		smp_boot_code.len() < BasePageSize::SIZE as usize,
		"SMP Boot Code is larger than a page"
	);
	debug!("SMP boot code is {} bytes long", smp_boot_code.len());

	// Identity-map the boot code page and copy over the code.
	debug!(
		"Mapping SMP boot code to physical and virtual address {:#X}",
		SMP_BOOT_CODE_ADDRESS
	);
	let mut flags = PageTableEntryFlags::empty();
	flags.normal().writable();
	paging::map::<BasePageSize>(
		SMP_BOOT_CODE_ADDRESS,
		PhysAddr(SMP_BOOT_CODE_ADDRESS.as_u64()),
		1,
		flags,
	);
	unsafe {
		ptr::copy_nonoverlapping(
			smp_boot_code.as_ptr(),
			SMP_BOOT_CODE_ADDRESS.as_mut_ptr(),
			smp_boot_code.len(),
		);
	}

	unsafe {
		// Pass the PML4 page table address to the boot code.
		*((SMP_BOOT_CODE_ADDRESS + SMP_BOOT_CODE_OFFSET_PML4).as_mut_ptr::<u32>()) =
			cr3().try_into().unwrap();
		// Set entry point
		debug!(
			"Set entry point for application processor to {:p}",
			start::_start as *const ()
		);
		*((SMP_BOOT_CODE_ADDRESS + SMP_BOOT_CODE_OFFSET_ENTRY).as_mut_ptr()) =
			start::_start as usize;
		*((SMP_BOOT_CODE_ADDRESS + SMP_BOOT_CODE_OFFSET_BOOTINFO).as_mut_ptr()) =
			raw_boot_info() as *const _ as u64;
	}

	// Now wake up each application processor.
	let apic_ids = unsafe { CPU_LOCAL_APIC_IDS.as_ref().unwrap() };
	let core_id = core_id();

	for (core_id_to_boot, &apic_id) in apic_ids.iter().enumerate() {
		let core_id_to_boot = core_id_to_boot as u32;
		if core_id_to_boot != core_id {
			unsafe {
				*((SMP_BOOT_CODE_ADDRESS + SMP_BOOT_CODE_OFFSET_CPU_ID).as_mut_ptr()) =
					core_id_to_boot;
			}
			let destination = u64::from(apic_id) << 32;

			debug!(
				"Waking up CPU {} with Local APIC ID {}",
				core_id_to_boot, apic_id
			);
			init_next_processor_variables(core_id_to_boot);

			// Save the current number of initialized CPUs.
			let current_processor_count = arch::get_processor_count();

			// Send an INIT IPI.
			local_apic_write(
				IA32_X2APIC_ICR,
				destination
					| APIC_ICR_LEVEL_TRIGGERED
					| APIC_ICR_LEVEL_ASSERT
					| APIC_ICR_DELIVERY_MODE_INIT,
			);
			processor::udelay(200);

			local_apic_write(
				IA32_X2APIC_ICR,
				destination | APIC_ICR_LEVEL_TRIGGERED | APIC_ICR_DELIVERY_MODE_INIT,
			);
			processor::udelay(10000);

			// Send a STARTUP IPI.
			local_apic_write(
				IA32_X2APIC_ICR,
				destination
					| APIC_ICR_DELIVERY_MODE_STARTUP
					| ((SMP_BOOT_CODE_ADDRESS.as_u64()) >> 12),
			);
			debug!("Waiting for it to respond");

			// Wait until the application processor has finished initializing.
			// It will indicate this by counting up cpu_online.
			while current_processor_count == arch::get_processor_count() {
				hint::spin_loop();
			}
		}
	}
}

#[cfg(feature = "smp")]
pub fn ipi_tlb_flush() {
	if arch::get_processor_count() > 1 {
		let apic_ids = unsafe { CPU_LOCAL_APIC_IDS.as_ref().unwrap() };
		let core_id = core_id();

		// Ensure that all memory operations have completed before issuing a TLB flush.
		unsafe {
			_mm_mfence();
		}

		// Send an IPI with our TLB Flush interrupt number to all other CPUs.
		irqsave(|| {
			for (core_id_to_interrupt, &apic_id) in apic_ids.iter().enumerate() {
				if core_id_to_interrupt != core_id.try_into().unwrap() {
					let destination = u64::from(apic_id) << 32;
					local_apic_write(
						IA32_X2APIC_ICR,
						destination
							| APIC_ICR_LEVEL_ASSERT | APIC_ICR_DELIVERY_MODE_FIXED
							| u64::from(TLB_FLUSH_INTERRUPT_NUMBER),
					);
				}
			}
		});
	}
}

/// Send an inter-processor interrupt to wake up a CPU Core that is in a HALT state.
#[allow(unused_variables)]
pub fn wakeup_core(core_id_to_wakeup: CoreId) {
	#[cfg(feature = "smp")]
	if core_id_to_wakeup != core_id() {
		irqsave(|| {
			let apic_ids = unsafe { CPU_LOCAL_APIC_IDS.as_ref().unwrap() };
			let local_apic_id = apic_ids[core_id_to_wakeup as usize];
			let destination = u64::from(local_apic_id) << 32;
			local_apic_write(
				IA32_X2APIC_ICR,
				destination
					| APIC_ICR_LEVEL_ASSERT
					| APIC_ICR_DELIVERY_MODE_FIXED
					| u64::from(WAKEUP_INTERRUPT_NUMBER),
			);
		});
	}
}

/// Translate the x2APIC MSR into an xAPIC memory address.
#[inline]
fn translate_x2apic_msr_to_xapic_address(x2apic_msr: u32) -> VirtAddr {
	unsafe { LOCAL_APIC_ADDRESS + ((x2apic_msr as u64 & 0xFF) << 4) }
}

fn local_apic_read(x2apic_msr: u32) -> u32 {
	if processor::supports_x2apic() {
		// x2APIC is simple, we can just read from the given MSR.
		unsafe { rdmsr(x2apic_msr) as u32 }
	} else {
		unsafe { *(translate_x2apic_msr_to_xapic_address(x2apic_msr).as_ptr::<u32>()) }
	}
}

fn ioapic_write(reg: u32, value: u32) {
	unsafe {
		core::ptr::write_volatile(IOAPIC_ADDRESS.as_mut_ptr::<u32>(), reg);
		core::ptr::write_volatile(
			(IOAPIC_ADDRESS + 4 * mem::size_of::<u32>()).as_mut_ptr::<u32>(),
			value,
		);
	}
}

fn ioapic_read(reg: u32) -> u32 {
	let value;

	unsafe {
		core::ptr::write_volatile(IOAPIC_ADDRESS.as_mut_ptr::<u32>(), reg);
		value =
			core::ptr::read_volatile((IOAPIC_ADDRESS + 4 * mem::size_of::<u32>()).as_ptr::<u32>());
	}

	value
}

fn ioapic_version() -> u32 {
	ioapic_read(IOAPIC_REG_VER) & 0xFF
}

fn ioapic_max_redirection_entry() -> u8 {
	((ioapic_read(IOAPIC_REG_VER) >> 16) & 0xFF) as u8
}

fn local_apic_write(x2apic_msr: u32, value: u64) {
	if processor::supports_x2apic() {
		// x2APIC is simple, we can just write the given value to the given MSR.
		unsafe {
			wrmsr(x2apic_msr, value);
		}
	} else {
		// Write the value.
		let value_ref = unsafe {
			&mut *(translate_x2apic_msr_to_xapic_address(x2apic_msr).as_mut_ptr::<u32>())
		};

		if x2apic_msr == IA32_X2APIC_ICR {
			// The ICR1 register in xAPIC mode also has a Delivery Status bit.
			// Wait until previous interrupt was deliverd.
			// This bit does not exist in x2APIC mode (cf. Intel Vol. 3A, 10.12.9).
			while (unsafe { core::ptr::read_volatile(value_ref) }
				& APIC_ICR_DELIVERY_STATUS_PENDING)
				> 0
			{
				spin_loop();
			}

			// Instead of a single 64-bit ICR register, xAPIC has two 32-bit registers (ICR1 and ICR2).
			// There is a gap between them and the destination field in ICR2 is also 8 bits instead of 32 bits.
			let destination = ((value >> 8) & 0xFF00_0000) as u32;
			let icr2 = unsafe { &mut *((LOCAL_APIC_ADDRESS + APIC_ICR2).as_mut_ptr::<u32>()) };
			*icr2 = destination;

			// The remaining data without the destination will now be written into ICR1.
		}

		*value_ref = value as u32;
	}
}

pub fn print_information() {
	infoheader!(" MULTIPROCESSOR INFORMATION ");
	infoentry!(
		"APIC in use",
		if processor::supports_x2apic() {
			"x2APIC"
		} else {
			"xAPIC"
		}
	);
	infoentry!("Initialized CPUs", arch::get_processor_count());
	infofooter!();
}
