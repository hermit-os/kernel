use alloc::alloc::alloc;
use alloc::vec::Vec;
use core::alloc::Layout;
#[cfg(feature = "smp")]
use core::arch::x86_64::_mm_mfence;
#[cfg(feature = "acpi")]
use core::fmt;
use core::hint::spin_loop;
use core::sync::atomic::Ordering;
use core::{cmp, mem, ptr};

use align_address::Align;
#[cfg(feature = "smp")]
use arch::x86_64::kernel::core_local::*;
use arch::x86_64::kernel::{interrupts, processor};
use hermit_sync::{OnceCell, SpinMutex, without_interrupts};
use memory_addresses::{AddrRange, PhysAddr, VirtAddr};
#[cfg(feature = "smp")]
use x86_64::registers::control::Cr3;
use x86_64::registers::model_specific::Msr;

use super::interrupts::IDT;
use crate::arch::x86_64::kernel::CURRENT_STACK_ADDRESS;
#[cfg(feature = "acpi")]
use crate::arch::x86_64::kernel::acpi;
use crate::arch::x86_64::mm::paging;
use crate::arch::x86_64::mm::paging::{
	BasePageSize, PageSize, PageTableEntryFlags, PageTableEntryFlagsExt,
};
use crate::arch::x86_64::swapgs;
use crate::config::*;
use crate::mm::virtualmem::{allocate_virtual, deallocate_virtual};
use crate::scheduler::CoreId;
use crate::{arch, env, scheduler};

/// APIC Location and Status (R/W) See Table 35-2. See Section 10.4.4, Local APIC  Status and Location.
const IA32_APIC_BASE: Msr = Msr::new(0x1b);

/// TSC Target of Local APIC s TSC Deadline Mode (R/W)  See Table 35-2
const IA32_TSC_DEADLINE: Msr = Msr::new(0x6e0);

/// x2APIC Task Priority register (R/W)
const IA32_X2APIC_TPR: u32 = 0x808;

/// x2APIC End of Interrupt. If ( CPUID.01H:ECX.\[bit 21\]  = 1 )
const IA32_X2APIC_EOI: u32 = 0x80b;

/// x2APIC Spurious Interrupt Vector register (R/W)
const IA32_X2APIC_SIVR: u32 = 0x80f;

/// Error Status Register. If ( CPUID.01H:ECX.\[bit 21\]  = 1 )
const IA32_X2APIC_ESR: u32 = 0x828;

/// x2APIC Interrupt Command register (R/W)
const IA32_X2APIC_ICR: u32 = 0x830;

/// x2APIC LVT Timer Interrupt register (R/W)
const IA32_X2APIC_LVT_TIMER: u32 = 0x832;

/// x2APIC LVT Thermal Sensor Interrupt register (R/W)
const IA32_X2APIC_LVT_THERMAL: u32 = 0x833;

/// x2APIC LVT Performance Monitor register (R/W)
const IA32_X2APIC_LVT_PMI: u32 = 0x834;

/// If ( CPUID.01H:ECX.\[bit 21\]  = 1 )
const IA32_X2APIC_LVT_LINT0: u32 = 0x835;

/// If ( CPUID.01H:ECX.\[bit 21\]  = 1 )
const IA32_X2APIC_LVT_LINT1: u32 = 0x836;

/// If ( CPUID.01H:ECX.\[bit 21\]  = 1 )
const IA32_X2APIC_LVT_ERROR: u32 = 0x837;

/// x2APIC Initial Count register (R/W)
const IA32_X2APIC_INIT_COUNT: u32 = 0x838;

/// x2APIC Current Count register (R/O)
const IA32_X2APIC_CUR_COUNT: u32 = 0x839;

/// x2APIC Divide Configuration register (R/W)
const IA32_X2APIC_DIV_CONF: u32 = 0x83e;

const MP_FLT_SIGNATURE: u32 = 0x5f50_4d5f;
const MP_CONFIG_SIGNATURE: u32 = 0x504d_4350;

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
const SMP_BOOT_CODE_ADDRESS: VirtAddr = VirtAddr::new(0x8000);

#[cfg(feature = "smp")]
const SMP_BOOT_CODE_OFFSET_ENTRY: u64 = 0x08;
#[cfg(feature = "smp")]
const SMP_BOOT_CODE_OFFSET_CPU_ID: u64 = SMP_BOOT_CODE_OFFSET_ENTRY + 0x08;
#[cfg(feature = "smp")]
const SMP_BOOT_CODE_OFFSET_PML4: u64 = SMP_BOOT_CODE_OFFSET_CPU_ID + 0x04;

const X2APIC_ENABLE: u64 = 1 << 10;

static LOCAL_APIC_ADDRESS: OnceCell<VirtAddr> = OnceCell::new();
static IOAPIC_ADDRESS: OnceCell<VirtAddr> = OnceCell::new();

/// Stores the Local APIC IDs of all CPUs. The index equals the Core ID.
/// Both numbers often match, but don't need to (e.g. when a core has been disabled).
static CPU_LOCAL_APIC_IDS: SpinMutex<Vec<u8>> = SpinMutex::new(Vec::new());

/// After calibration, initialize the APIC Timer with this counter value to let it fire an interrupt
/// after 1 microsecond.
static CALIBRATED_COUNTER_VALUE: OnceCell<u64> = OnceCell::new();

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
	ty: u8,
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
	ty: u8,
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

#[cfg(feature = "acpi")]
#[repr(C, packed)]
struct ProcessorLocalApicRecord {
	acpi_processor_id: u8,
	apic_id: u8,
	flags: u32,
}

#[cfg(feature = "acpi")]
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

#[cfg(feature = "acpi")]
#[repr(C, packed)]
struct IoApicRecord {
	id: u8,
	reserved: u8,
	address: u32,
	global_system_interrupt_base: u32,
}

#[cfg(feature = "acpi")]
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
extern "x86-interrupt" fn tlb_flush_handler(stack_frame: interrupts::ExceptionStackFrame) {
	swapgs(&stack_frame);
	debug!("Received TLB Flush Interrupt");
	increment_irq_counter(TLB_FLUSH_INTERRUPT_NUMBER);
	let (frame, val) = Cr3::read_raw();
	unsafe {
		Cr3::write_raw(frame, val);
	}
	eoi();
	swapgs(&stack_frame);
}

extern "x86-interrupt" fn error_interrupt_handler(stack_frame: interrupts::ExceptionStackFrame) {
	swapgs(&stack_frame);
	error!("APIC LVT Error Interrupt");
	error!("ESR: {:#X}", local_apic_read(IA32_X2APIC_ESR));
	error!("{stack_frame:#?}");
	eoi();
	scheduler::abort();
}

extern "x86-interrupt" fn spurious_interrupt_handler(stack_frame: interrupts::ExceptionStackFrame) {
	swapgs(&stack_frame);
	error!("Spurious Interrupt: {stack_frame:#?}");
	scheduler::abort();
}

#[cfg(feature = "smp")]
extern "x86-interrupt" fn wakeup_handler(stack_frame: interrupts::ExceptionStackFrame) {
	swapgs(&stack_frame);
	use crate::scheduler::PerCoreSchedulerExt;

	debug!("Received Wakeup Interrupt");
	increment_irq_counter(WAKEUP_INTERRUPT_NUMBER);
	let core_scheduler = core_scheduler();
	core_scheduler.check_input();
	eoi();
	if core_scheduler.is_scheduling() {
		core_scheduler.reschedule();
	}
	swapgs(&stack_frame);
}

#[inline]
pub fn add_local_apic_id(id: u8) {
	CPU_LOCAL_APIC_IDS.lock().push(id);
}

#[cfg(feature = "smp")]
pub fn local_apic_id_count() -> u32 {
	CPU_LOCAL_APIC_IDS.lock().len() as u32
}

fn init_ioapic_address(phys_addr: PhysAddr) {
	if env::is_uefi() {
		// UEFI systems have already id mapped everything, so we can just set the physical address as the virtual one
		IOAPIC_ADDRESS
			.set(VirtAddr::new(phys_addr.as_u64()))
			.unwrap();
	} else {
		let ioapic_address =
			allocate_virtual(BasePageSize::SIZE as usize, BasePageSize::SIZE as usize).unwrap();
		IOAPIC_ADDRESS.set(ioapic_address).unwrap();
		debug!("Mapping IOAPIC at {phys_addr:p} to virtual address {ioapic_address:p}");

		let mut flags = PageTableEntryFlags::empty();
		flags.device().writable().execute_disable();
		paging::map::<BasePageSize>(ioapic_address, phys_addr, 1, flags);
	}
}

#[cfg(not(feature = "acpi"))]
fn detect_from_acpi() -> Result<PhysAddr, ()> {
	// dummy implementation if acpi support is disabled
	Err(())
}

#[cfg(feature = "acpi")]
fn detect_from_acpi() -> Result<PhysAddr, ()> {
	// Get the Multiple APIC Description Table (MADT) from the ACPI information and its specific table header.
	let madt = acpi::get_madt().ok_or(())?;
	let madt_header =
		unsafe { &*(ptr::with_exposed_provenance::<AcpiMadtHeader>(madt.table_start_address())) };

	// Jump to the actual table entries (after the table header).
	let mut current_address = madt.table_start_address() + mem::size_of::<AcpiMadtHeader>();

	// Loop through all table entries.
	while current_address < madt.table_end_address() {
		let record =
			unsafe { &*(ptr::with_exposed_provenance::<AcpiMadtRecordHeader>(current_address)) };
		current_address += mem::size_of::<AcpiMadtRecordHeader>();

		match record.entry_type {
			0 => {
				// Processor Local APIC
				let processor_local_apic_record = unsafe {
					&*(ptr::with_exposed_provenance::<ProcessorLocalApicRecord>(current_address))
				};
				debug!("Found Processor Local APIC record: {processor_local_apic_record}");

				if processor_local_apic_record.flags & CPU_FLAG_ENABLED > 0 {
					add_local_apic_id(processor_local_apic_record.apic_id);
				}
			}
			1 => {
				// I/O APIC
				let ioapic_record =
					unsafe { &*(ptr::with_exposed_provenance::<IoApicRecord>(current_address)) };
				debug!("Found I/O APIC record: {ioapic_record}");

				init_ioapic_address(PhysAddr::new(ioapic_record.address.into()));
			}
			_ => {
				// Just ignore other entries for now.
			}
		}

		current_address += record.length as usize - mem::size_of::<AcpiMadtRecordHeader>();
	}

	// Successfully derived all information from the MADT.
	// Return the physical address of the Local APIC.
	Ok(PhysAddr::new(madt_header.local_apic_address.into()))
}

/// Helper function to search Floating Pointer Structure of the Multiprocessing Specification
fn search_mp_floating(memory_range: AddrRange<PhysAddr>) -> Result<&'static ApicMP, ()> {
	let virtual_address =
		allocate_virtual(BasePageSize::SIZE as usize, BasePageSize::SIZE as usize).unwrap();

	for current_address in memory_range.iter().step_by(BasePageSize::SIZE as usize) {
		let mut flags = PageTableEntryFlags::empty();
		flags.normal().writable();
		paging::map::<BasePageSize>(
			virtual_address,
			current_address.align_down(BasePageSize::SIZE),
			1,
			flags,
		);

		for i in 0..BasePageSize::SIZE / 4 {
			let mut tmp: *const u32 = virtual_address.as_ptr();
			tmp = unsafe { tmp.offset(i.try_into().unwrap()) };
			let apic_mp = unsafe { &*tmp.cast::<ApicMP>() };
			if apic_mp.signature == MP_FLT_SIGNATURE
				&& !(apic_mp.version > 4 || apic_mp.features[0] != 0)
			{
				return Ok(apic_mp);
			}
		}
	}

	// frees obsolete virtual memory region for MMIO devices
	unsafe {
		deallocate_virtual(virtual_address, BasePageSize::SIZE as usize);
	}

	Err(())
}

/// Helper function to detect APIC by the Multiprocessor Specification
fn detect_from_mp() -> Result<PhysAddr, ()> {
	let mp_float = if let Ok(mpf) = search_mp_floating(
		AddrRange::new(PhysAddr::new(0x9f000u64), PhysAddr::new(0xa0000u64)).unwrap(),
	) {
		Ok(mpf)
	} else if let Ok(mpf) = search_mp_floating(
		AddrRange::new(PhysAddr::new(0xf0000u64), PhysAddr::new(0x10_0000u64)).unwrap(),
	) {
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

	let virtual_address =
		allocate_virtual(BasePageSize::SIZE as usize, BasePageSize::SIZE as usize).unwrap();

	let mut flags = PageTableEntryFlags::empty();
	flags.normal().writable();
	paging::map::<BasePageSize>(
		virtual_address,
		PhysAddr::from((mp_float.mp_config as usize).align_down(BasePageSize::SIZE as usize)),
		1,
		flags,
	);

	let mut addr: usize =
		(virtual_address | (u64::from(mp_float.mp_config) & (BasePageSize::SIZE - 1))) as usize;
	let mp_config: &ApicConfigTable = unsafe { &*(ptr::with_exposed_provenance(addr)) };
	if mp_config.signature != MP_CONFIG_SIGNATURE {
		warn!("MP config table invalid!");
		unsafe {
			deallocate_virtual(virtual_address, BasePageSize::SIZE as usize);
		}
		return Err(());
	}

	if mp_config.entry_count == 0 {
		warn!("No MP table entries, guessing IOAPIC...");
		let default_address = PhysAddr::new(0xfec0_0000);

		init_ioapic_address(default_address);
	} else {
		// entries starts directly after the config table
		addr += mem::size_of::<ApicConfigTable>();
		for _i in 0..mp_config.entry_count {
			match unsafe { *(ptr::with_exposed_provenance::<u8>(addr)) } {
				// CPU entry
				0 => {
					let cpu_entry: &ApicProcessorEntry =
						unsafe { &*(ptr::with_exposed_provenance(addr)) };
					if cpu_entry.cpu_flags & 0x01 == 0x01 {
						add_local_apic_id(cpu_entry.id);
					}
					addr += mem::size_of::<ApicProcessorEntry>();
				}
				// IO-APIC entry
				2 => {
					let io_entry: &ApicIoEntry = unsafe { &*(ptr::with_exposed_provenance(addr)) };
					let ioapic = PhysAddr::new(io_entry.addr.into());
					info!("IOAPIC found at {ioapic:p}");

					init_ioapic_address(ioapic);

					addr += mem::size_of::<ApicIoEntry>();
				}
				_ => {
					addr += 8;
				}
			}
		}
	}

	Ok(PhysAddr::new(mp_config.lapic.into()))
}

fn default_apic() -> PhysAddr {
	let default_address = PhysAddr::new(0xfee0_0000);

	warn!("Using default APIC address: {default_address:p}");

	// currently, uhyve doesn't support an IO-APIC
	if !env::is_uhyve() {
		init_ioapic_address(default_address);
	}

	default_address
}

pub fn eoi() {
	local_apic_write(IA32_X2APIC_EOI, APIC_EOI_ACK);
}

pub fn init() {
	// Detect CPUs and APICs.
	let local_apic_physical_address = if env::is_uhyve() {
		default_apic()
	} else {
		detect_from_acpi()
			.or_else(|()| detect_from_mp())
			.unwrap_or_else(|()| default_apic())
	};

	// Initialize x2APIC or xAPIC, depending on what's available.
	if processor::supports_x2apic() {
		init_x2apic();
	} else if env::is_uefi() {
		// already id mapped in UEFI systems, just use the physical address as virtual one
		LOCAL_APIC_ADDRESS
			.set(VirtAddr::new(local_apic_physical_address.as_u64()))
			.unwrap();
	} else {
		// We use the traditional xAPIC mode available on all x86-64 CPUs.
		// It uses a mapped page for communication.
		let local_apic_address =
			allocate_virtual(BasePageSize::SIZE as usize, BasePageSize::SIZE as usize).unwrap();

		let mut flags = PageTableEntryFlags::empty();
		flags.device().writable().execute_disable();
		paging::map::<BasePageSize>(local_apic_address, local_apic_physical_address, 1, flags);
	}

	// Set gates to ISRs for the APIC interrupts we are going to enable.
	unsafe {
		let mut idt = IDT.lock();
		idt[ERROR_INTERRUPT_NUMBER]
			.set_handler_fn(error_interrupt_handler)
			.set_stack_index(0);
		idt[SPURIOUS_INTERRUPT_NUMBER]
			.set_handler_fn(spurious_interrupt_handler)
			.set_stack_index(0);
		#[cfg(feature = "smp")]
		{
			idt[TLB_FLUSH_INTERRUPT_NUMBER]
				.set_handler_fn(tlb_flush_handler)
				.set_stack_index(0);
			interrupts::add_irq_name(TLB_FLUSH_INTERRUPT_NUMBER - 32, "TLB flush");
			idt[WAKEUP_INTERRUPT_NUMBER]
				.set_handler_fn(wakeup_handler)
				.set_stack_index(0);
			interrupts::add_irq_name(WAKEUP_INTERRUPT_NUMBER - 32, "Wakeup");
		}
	}

	// Initialize interrupt handling over APIC.
	// All interrupts of the PIC have already been masked, so it doesn't need to be disabled again.
	init_local_apic();

	if !processor::supports_tsc_deadline() {
		// We have an older APIC Timer without TSC Deadline support, which has a maximum timeout
		// and needs to be calibrated.
		calibrate_timer();
	}

	// currently, IO-APIC isn't supported by uhyve
	if !env::is_uhyve() {
		// initialize IO-APIC
		init_ioapic();
	}
}

fn init_ioapic() {
	let max_entry = ioapic_max_redirection_entry() + 1;
	info!("IOAPIC v{} has {} entries", ioapic_version(), max_entry);

	// now lets turn everything else on
	for i in 0..max_entry {
		// Turn off the Programmable Interrupt Timer Interrupt (IRQ 0) and
		// the Real Time Clock (IRQ 2).
		let enabled = !matches!(i, 0 | 2);
		ioapic_set_interrupt(i, 0, enabled);
	}
}

fn ioapic_set_interrupt(irq: u8, apicid: u8, enabled: bool) {
	assert!(irq <= 24);

	let off = u32::from(irq * 2);
	let ioredirect_upper = u32::from(apicid) << 24;
	let mut ioredirect_lower = u32::from(0x20 + irq);
	if !enabled {
		debug!("Disabling irq {irq}");
		ioredirect_lower |= 1 << 16;
	}

	ioapic_write(IOAPIC_REG_TABLE + off, ioredirect_lower);
	ioapic_write(IOAPIC_REG_TABLE + off + 1, ioredirect_upper);
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
	// used throughout all of Hermit. Wait 30ms for accuracy.
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
	let calibrated_counter_value =
		(u64::from(u32::MAX - local_apic_read(IA32_X2APIC_CUR_COUNT))) / microseconds;
	CALIBRATED_COUNTER_VALUE
		.set(calibrated_counter_value)
		.unwrap();
	debug!(
		"Calibrated APIC Timer with a counter value of {calibrated_counter_value} for 1 microsecond",
	);
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
			let mut ia32_tsc_deadline = IA32_TSC_DEADLINE;
			unsafe {
				ia32_tsc_deadline.write(tsc_deadline);
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
				CALIBRATED_COUNTER_VALUE.get().unwrap() * ticks,
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
	without_interrupts(|| {
		__set_oneshot_timer(wakeup_time);
	});
}

pub fn init_x2apic() {
	debug!("Enable x2APIC support");
	// The CPU supports the modern x2APIC mode, which uses MSRs for communication.
	// Enable it.
	let mut msr = IA32_APIC_BASE;
	let mut apic_base = unsafe { msr.read() };
	apic_base |= X2APIC_ENABLE;
	unsafe {
		msr.write(apic_base);
	}
}

/// Initialize the required _start variables for the next CPU to be booted.
pub fn init_next_processor_variables() {
	// Allocate stack for the CPU and pass the addresses.
	let layout = Layout::from_size_align(KERNEL_STACK_SIZE, BasePageSize::SIZE as usize).unwrap();
	let stack = unsafe { alloc(layout) };
	assert!(!stack.is_null());
	CURRENT_STACK_ADDRESS.store(stack, Ordering::Relaxed);
}

/// Boot all Application Processors
/// This algorithm is derived from Intel MultiProcessor Specification 1.4, B.4, but testing has shown
/// that a second STARTUP IPI and setting the BIOS Reset Vector are no longer necessary.
/// This is partly confirmed by <https://wiki.osdev.org/Symmetric_Multiprocessing>
#[cfg(all(target_os = "none", feature = "smp"))]
pub fn boot_application_processors() {
	use core::hint;

	use x86_64::structures::paging::Translate;

	use super::start;

	let smp_boot_code = include_bytes!(concat!(core::env!("OUT_DIR"), "/boot.bin"));

	// We shouldn't have any problems fitting the boot code into a single page, but let's better be sure.
	assert!(
		smp_boot_code.len() < BasePageSize::SIZE as usize,
		"SMP Boot Code is larger than a page"
	);
	debug!("SMP boot code is {} bytes long", smp_boot_code.len());

	if env::is_uefi() {
		// Since UEFI already provides identity-mapped pagetables, we only have to sanity-check the identity mapping
		let pt = unsafe { crate::arch::mm::paging::identity_mapped_page_table() };
		let virt_addr = SMP_BOOT_CODE_ADDRESS;
		let phys_addr = pt.translate_addr(virt_addr.into()).unwrap();
		assert_eq!(phys_addr.as_u64(), virt_addr.as_u64());
	} else {
		// Identity-map the boot code page and copy over the code.
		debug!("Mapping SMP boot code to physical and virtual address {SMP_BOOT_CODE_ADDRESS:p}");
		let mut flags = PageTableEntryFlags::empty();
		flags.normal().writable();
		paging::map::<BasePageSize>(
			SMP_BOOT_CODE_ADDRESS,
			PhysAddr::new(SMP_BOOT_CODE_ADDRESS.as_u64()),
			1,
			flags,
		);
	}
	unsafe {
		ptr::copy_nonoverlapping(
			smp_boot_code.as_ptr(),
			SMP_BOOT_CODE_ADDRESS.as_mut_ptr(),
			smp_boot_code.len(),
		);
	}

	unsafe {
		let (frame, val) = Cr3::read_raw();
		let value = frame.start_address().as_u64() | u64::from(val);
		// Pass the PML4 page table address to the boot code.
		*((SMP_BOOT_CODE_ADDRESS + SMP_BOOT_CODE_OFFSET_PML4).as_mut_ptr::<u32>()) =
			value.try_into().unwrap();
		// Set entry point
		debug!(
			"Set entry point for application processor to {:p}",
			start::_start as *const ()
		);
		ptr::write_unaligned(
			(SMP_BOOT_CODE_ADDRESS + SMP_BOOT_CODE_OFFSET_ENTRY).as_mut_ptr(),
			start::_start as usize,
		);
	}

	// Now wake up each application processor.
	let apic_ids = CPU_LOCAL_APIC_IDS.lock();
	let core_id = core_id();

	for (core_id_to_boot, &apic_id) in apic_ids.iter().enumerate() {
		let core_id_to_boot = core_id_to_boot as u32;
		if core_id_to_boot != core_id {
			unsafe {
				*((SMP_BOOT_CODE_ADDRESS + SMP_BOOT_CODE_OFFSET_CPU_ID).as_mut_ptr()) =
					core_id_to_boot;
			}
			let destination = u64::from(apic_id) << 32;

			debug!("Waking up CPU {core_id_to_boot} with Local APIC ID {apic_id}");
			init_next_processor_variables();

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

	print_information();
}

#[cfg(feature = "smp")]
pub fn ipi_tlb_flush() {
	if arch::get_processor_count() > 1 {
		let apic_ids = CPU_LOCAL_APIC_IDS.lock();
		let core_id = core_id();

		// Ensure that all memory operations have completed before issuing a TLB flush.
		unsafe {
			_mm_mfence();
		}

		// Send an IPI with our TLB Flush interrupt number to all other CPUs.
		without_interrupts(|| {
			for (core_id_to_interrupt, &apic_id) in apic_ids.iter().enumerate() {
				if core_id_to_interrupt != usize::try_from(core_id).unwrap() {
					let destination = u64::from(apic_id) << 32;
					local_apic_write(
						IA32_X2APIC_ICR,
						destination
							| APIC_ICR_LEVEL_ASSERT
							| APIC_ICR_DELIVERY_MODE_FIXED
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
	#[cfg(all(feature = "smp", not(feature = "idle-poll")))]
	if core_id_to_wakeup != core_id()
		&& !crate::processor::supports_mwait()
		&& crate::scheduler::take_core_hlt_state(core_id_to_wakeup)
	{
		without_interrupts(|| {
			let apic_ids = CPU_LOCAL_APIC_IDS.lock();
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
	*LOCAL_APIC_ADDRESS.get().unwrap() + ((u64::from(x2apic_msr) & 0xff) << 4)
}

fn local_apic_read(x2apic_msr: u32) -> u32 {
	if processor::supports_x2apic() {
		// x2APIC is simple, we can just read from the given MSR.
		unsafe { Msr::new(x2apic_msr).read() as u32 }
	} else {
		unsafe { *(translate_x2apic_msr_to_xapic_address(x2apic_msr).as_ptr::<u32>()) }
	}
}

fn ioapic_write(reg: u32, value: u32) {
	unsafe {
		core::ptr::write_volatile(IOAPIC_ADDRESS.get().unwrap().as_mut_ptr::<u32>(), reg);
		core::ptr::write_volatile(
			(*IOAPIC_ADDRESS.get().unwrap() + 4 * mem::size_of::<u32>()).as_mut_ptr::<u32>(),
			value,
		);
	}
}

fn ioapic_read(reg: u32) -> u32 {
	let value;

	unsafe {
		core::ptr::write_volatile(IOAPIC_ADDRESS.get().unwrap().as_mut_ptr::<u32>(), reg);
		value = core::ptr::read_volatile(
			(*IOAPIC_ADDRESS.get().unwrap() + 4 * mem::size_of::<u32>()).as_ptr::<u32>(),
		);
	}

	value
}

fn ioapic_version() -> u32 {
	ioapic_read(IOAPIC_REG_VER) & 0xff
}

fn ioapic_max_redirection_entry() -> u8 {
	((ioapic_read(IOAPIC_REG_VER) >> 16) & 0xff) as u8
}

fn local_apic_write(x2apic_msr: u32, value: u64) {
	if processor::supports_x2apic() {
		// x2APIC is simple, we can just write the given value to the given MSR.
		unsafe {
			Msr::new(x2apic_msr).write(value);
		}
	} else {
		// Write the value.
		let value_ref = unsafe {
			&mut *(translate_x2apic_msr_to_xapic_address(x2apic_msr).as_mut_ptr::<u32>())
		};

		if x2apic_msr == IA32_X2APIC_ICR {
			// The ICR1 register in xAPIC mode also has a Delivery Status bit.
			// Wait until previous interrupt was delivered.
			// This bit does not exist in x2APIC mode (cf. Intel Vol. 3A, 10.12.9).
			while (unsafe { core::ptr::read_volatile(value_ref) }
				& APIC_ICR_DELIVERY_STATUS_PENDING)
				> 0
			{
				spin_loop();
			}

			// Instead of a single 64-bit ICR register, xAPIC has two 32-bit registers (ICR1 and ICR2).
			// There is a gap between them and the destination field in ICR2 is also 8 bits instead of 32 bits.
			let destination = ((value >> 8) & 0xff00_0000) as u32;
			let icr2 = unsafe {
				&mut *((*LOCAL_APIC_ADDRESS.get().unwrap() + APIC_ICR2).as_mut_ptr::<u32>())
			};
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
