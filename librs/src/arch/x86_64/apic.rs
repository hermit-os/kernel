// Copyright (c) 2017 Colin Finck, RWTH Aachen University
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

include!(concat!(env!("CARGO_TARGET_DIR"), "/config.rs"));
include!(concat!(env!("CARGO_TARGET_DIR"), "/smp_boot_code.rs"));

use core::fmt;
use alloc::boxed::Box;
use alloc::vec::Vec;
use arch::x86_64::idt;
use arch::x86_64::irq;
use arch::x86_64::mm::paging;
use arch::x86_64::mm::paging::{BasePageSize, PageSize, PageTableEntryFlags};
use arch::x86_64::mm::virtualmem;
use arch::x86_64::percore::*;
use arch::x86_64::processor;
use core::sync::atomic::spin_loop_hint;
use core::{mem, ptr, str, u32};
use environment;
use mm;
use scheduler;
use x86::shared::control_regs::*;
use x86::shared::msr::*;


extern "C" {
	static cpu_online: u32;
	static mut current_stack_address: usize;
	static mut current_percore_address: usize;
}

const APIC_ICR2: usize = 0x0310;

const APIC_DIV_CONF_DIVIDE_BY_128: u64      = 0b1010;
const APIC_EOI_ACK: u64                     = 0;
const APIC_ICR_DELIVERY_MODE_FIXED: u64     = 0x000;
const APIC_ICR_DELIVERY_MODE_INIT: u64      = 0x500;
const APIC_ICR_DELIVERY_MODE_STARTUP: u64   = 0x600;
const APIC_ICR_DELIVERY_STATUS_PENDING: u32 = 1 << 12;
const APIC_ICR_LEVEL_TRIGGERED: u64         = 1 << 15;
const APIC_ICR_LEVEL_ASSERT: u64            = 1 << 14;
const APIC_LVT_MASK: u64                    = 1 << 16;
const APIC_SIVR_ENABLED: u64                = 1 << 8;

/// Register index: ID
#[allow(dead_code)]
const IOAPIC_REG_ID: u32					= 0x0000;
/// Register index: version
const IOAPIC_REG_VER: u32					= 0x0001;
/// Redirection table base
const IOAPIC_REG_TABLE: u32					= 0x0010;

const TLB_FLUSH_INTERRUPT_NUMBER: u8 = 112;
const WAKEUP_INTERRUPT_NUMBER: u8    = 121;
pub const TIMER_INTERRUPT_NUMBER: u8 = 123;
const ERROR_INTERRUPT_NUMBER: u8     = 126;
const SPURIOUS_INTERRUPT_NUMBER: u8  = 127;

/// Physical and virtual memory address for our SMP boot code.
///
/// While our boot processor is already in x86-64 mode, application processors boot up in 16-bit real mode
/// and need an address in the CS:IP addressing scheme to jump to.
/// The CS:IP addressing scheme is limited to 2^20 bytes (= 1 MiB).
const SMP_BOOT_CODE_ADDRESS: usize = 0x8000;

const SMP_BOOT_CODE_OFFSET_PML4: usize = 0x04;

const X2APIC_ENABLE: u64 = 1 << 10;

static mut LOCAL_APIC_ADDRESS: usize = 0;
static mut IOAPIC_ADDRESS: usize = 0;

/// Stores the Local APIC IDs of all CPUs.
/// As Rust currently implements no way of zero-initializing a global Vec in a no_std environment,
/// we have to encapsulate it in an Option...
static mut CPU_LOCAL_APIC_IDS: Option<Vec<u8>> = None;

/// After calibration, initialize the APIC Timer with this counter value to let it fire an interrupt
/// after a single tick of the timer specified by processor::TIMER_FREQUENCY.
static mut CALIBRATED_COUNTER_VALUE: usize = 0;


#[repr(C, packed)]
struct MultiProcessorFloatingPointer {
	signature: [u8; 4],
	configuration_table_ptr: u32,
	length: u8,
	spec_rev: u8,
	checksum: u8,
	features: [u8; 5],
}

#[repr(C, packed)]
struct MultiProcessorConfigurationTableHeader {
	signature: [u8; 4],
	base_table_length: u16,
	spec_rev: u8,
	checksum: u8,
	oem_id: [u8; 8],
	product_id: [u8; 12],
	oem_table_ptr: u32,
	oem_table_size: u16,
	entry_count: u16,
	local_apic_address: u32,
	extended_table_length: u16,
	extended_table_checksum: u8,
	reserved: u8,
}

#[repr(C, packed)]
struct CpuEntry {
	entry_type: u8,
	local_apic_id: u8,
	local_apic_version: u8,
	flags: u8,
	signature: u32,
	features: u32,
	reserved: u64,
}

impl fmt::Display for CpuEntry {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		write!(f, "{{ entry type: {}, ", self.entry_type)?;
		write!(f, "local_apic_id: {}, ", self.local_apic_id)?;
		write!(f, "local_apic_version: {}, ", self.local_apic_version)?;
		write!(f, "flags: {}, ", self.flags)?;
		unsafe {write!(f, "signature: 0x{:x}, ", self.signature)?; }
		unsafe { write!(f, "features: 0x{:x} }}", self.features) }
	}
}

const CPU_FLAG_ENABLED: u8        = 1 << 0;
const CPU_FLAG_BOOT_PROCESSOR: u8 = 1 << 1;

#[repr(C, packed)]
struct BusEntry {
	entry_type: u8,
	id: u8,
	type_string: [u8; 6],
}

impl fmt::Display for BusEntry {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		write!(f, "{{ entry type: {}, ", self.entry_type)?;
		write!(f, "id: {}, ", self.id)?;
		write!(f, "type_string: {:?} }}", self.type_string)
	}
}

#[repr(C, packed)]
struct IoApicEntry {
	entry_type: u8,
	id: u8,
	version: u8,
	flags: u8,
	address: u32,
}

impl fmt::Display for IoApicEntry {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		write!(f, "{{ entry type: {}, ", self.entry_type)?;
		write!(f, "id: {}, ", self.id)?;
		write!(f, "version: {}, ", self.version)?;
		write!(f, "flags: {}, ", self.flags)?;
		unsafe { write!(f, "address: 0x{:x} }}", self.address) }
	}
}

#[repr(C, packed)]
struct IoInterruptEntry {
	entry_type: u8,
	interrupt_type: u8,
	flags: u16,
	source_bus_id: u8,
	source_bus_irq: u8,
	destination_ioapic_id: u8,
	destination_ioapic_intin: u8,
}

impl fmt::Display for IoInterruptEntry {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		write!(f, "{{ entry type: {}, ", self.entry_type)?;
		write!(f, "interrupt_type: {}, ", self.interrupt_type)?;
		unsafe { write!(f, "flags: 0x{:x}, ", self.flags)?; }
		write!(f, "source_bus_id: {}, ", self.source_bus_id)?;
		write!(f, "source_bus_irq: {}, ", self.source_bus_irq)?;
		write!(f, "destination_ioapic_id: {}, ", self.destination_ioapic_id)?;
		write!(f, "destination_ioapic_intin: {} }}", self.destination_ioapic_intin)
	}
}

#[repr(C, packed)]
struct LocalInterruptEntry {
	entry_type: u8,
	interrupt_type: u8,
	flags: u16,
	source_bus_id: u8,
	source_bus_irq: u8,
	destination_local_apic_id: u8,
	destination_local_apic_lintin: u8,
}

impl fmt::Display for LocalInterruptEntry {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		write!(f, "{{ entry type: {}, ", self.entry_type)?;
		write!(f, "interrupt_type: {}, ", self.interrupt_type)?;
		unsafe { write!(f, "flags: 0x{:x}, ", self.flags)?; }
		write!(f, "source_bus_id: {}, ", self.source_bus_id)?;
		write!(f, "source_bus_irq: {}, ", self.source_bus_irq)?;
		write!(f, "destination_local_apic_id: {}, ", self.destination_local_apic_id)?;
		write!(f, "destination_local_apic_lintin: {} }}", self.destination_local_apic_lintin)
	}
}

extern "x86-interrupt" fn tlb_flush_handler(_stack_frame: &mut irq::ExceptionStackFrame) {
	debug!("Received TLB Flush Interrupt");
	unsafe { cr3_write(cr3()); }
	eoi();
}

extern "x86-interrupt" fn error_interrupt_handler(stack_frame: &mut irq::ExceptionStackFrame) {
	error!("APIC LVT Error Interrupt");
	error!("ESR: {:#X}", local_apic_read(IA32_X2APIC_ESR));
	error!("{:#?}", stack_frame);
	eoi();
	scheduler::abort();
}

extern "x86-interrupt" fn spurious_interrupt_handler(stack_frame: &mut irq::ExceptionStackFrame) {
	error!("Spurious Interrupt: {:#?}", stack_frame);
	scheduler::abort();
}

extern "x86-interrupt" fn wakeup_handler(_stack_frame: &mut irq::ExceptionStackFrame) {
	debug!("Received Wakeup Interrupt");
	eoi();
}

fn detect_multiprocessor_configuration_table(start_address: usize, end_address: usize) -> Result<usize, ()> {
	// Trigger page mapping in the first iteration!
	let mut current_page = 0;

	// Look for the MultiProcessor Floating Pointer Structure in all possible 4-byte aligned addresses within this range.
	for current_address in (start_address..end_address).step_by(4) {
		// Have we crossed a page boundary in the last iteration?
		if current_address / BasePageSize::SIZE > current_page {
			// Identity-map this possible page of the MultiProcessor Floating Pointer Structure.
			paging::map::<BasePageSize>(current_address, current_address, 1, PageTableEntryFlags::CACHE_DISABLE | PageTableEntryFlags::EXECUTE_DISABLE, false);
			current_page = current_address / BasePageSize::SIZE;
		}

		// Verify the signature to find out if this is really a MultiProcessor Floating Pointer Structure.
		let mp_floating = unsafe { & *(current_address as *const MultiProcessorFloatingPointer) };
		let signature = unsafe { str::from_utf8_unchecked(&mp_floating.signature) };

		if signature == "_MP_" {
			// It is, so verify that it conforms to MultiProcessor Specification 1.4 and comes with a MultiProcessor Configuration Table.
			assert!(
				mp_floating.spec_rev == 4,
				"MultiProcessor Specification 1.4 is required, but the system reports version 1.{} (according to structure at {:#X})", mp_floating.spec_rev, current_address
			);
			assert!(
				mp_floating.length == 1,
				"MultiProcessor Floating Pointer Structure at {:#X} has invalid length {:#X}", current_address, mp_floating.length
			);
			assert!(
				mp_floating.features[0] == 0,
				"A MultiProcessor Configuration Table is required, but the system relies on a default configuration (according to structure at {:#X})", current_address
			);

			// We were successful! Return a pointer to the MultiProcessor Configuration Table.
			debug!("Found version 1.4 MultiProcessor Floating Pointer Structure at {:#X}, with Configuration Table at {:#X}.", current_address, mp_floating.configuration_table_ptr as usize);
			return Ok(mp_floating.configuration_table_ptr as usize);
		}
	}

	// We found no MultiProcessor Floating Pointer Structure.
	Err(())
}

fn detect_from_multiprocessor_specification() -> Result<usize, ()> {
	// We require a system conforming to Intel MultiProcessor Specification 1.4.
	// The specification gives three locations where the MultiProcessor Floating Pointer Structure can be found.
	// However, experiments have shown that only searching between 0xF_0000 and 0xF_FFFF is sufficient.
	let mut current_address = detect_multiprocessor_configuration_table(0xF_0000, 0xF_FFFF)?;
	let mut current_page = current_address / BasePageSize::SIZE;

	// Identity-map the MultiProcessor Configuration Table.
	// Require it to be below the kernel start address (2 MiB), because everything above is managed by HermitCore without gaps.
	assert!(current_address < mm::kernel_start_address(), "MultiProcessor Configuration Table address {:#X} is not < KERNEL_START_ADDRESS", current_address);
	let mut map_address = align_down!(current_address, BasePageSize::SIZE);
	paging::map::<BasePageSize>(map_address, map_address, 1, PageTableEntryFlags::CACHE_DISABLE | PageTableEntryFlags::EXECUTE_DISABLE, false);

	// Verify the signature to find out if this is really a MultiProcessor Configuration Table.
	let mp_config_header = unsafe { & *(current_address as *const MultiProcessorConfigurationTableHeader) };
	let signature = unsafe { str::from_utf8_unchecked(&mp_config_header.signature) };
	assert!(signature == "PCMP", "MultiProcessor Configuration Table at {:#X} has invalid signature", current_address);

	// Jump to the actual table entries (after the table header).
	current_address += mem::size_of::<MultiProcessorConfigurationTableHeader>();

	// Initialize an empty vector for the Local APIC IDs of all CPUs.
	let local_apic_ids = unsafe {
		CPU_LOCAL_APIC_IDS = Some(Vec::new());
		CPU_LOCAL_APIC_IDS.as_mut().unwrap()
	};

	// Loop through all table entries.
	for _i in 0..mp_config_header.entry_count {
		// Have we crossed a page boundary in the last iteration?
		if current_address / BasePageSize::SIZE > current_page {
			// Then we need to map another page for the MultiProcessor Configuration Table.
			map_address = align_down!(current_address, BasePageSize::SIZE);
			assert!(map_address < mm::kernel_start_address(), "Additional MultiProcessor Configuration Table address {:#X} is not < KERNEL_START_ADDRESS", map_address);
			paging::map::<BasePageSize>(map_address, map_address, 1, PageTableEntryFlags::CACHE_DISABLE | PageTableEntryFlags::EXECUTE_DISABLE, false);

			current_page += 1;
		}

		// Check what entry we have.
		let entry_type = unsafe { & *(current_address as *const u8) };
		match entry_type {
			&0 => {
				// CPU
				let cpu = unsafe { & *(current_address as *const CpuEntry) };
				debug!("Found CPU entry: {}", cpu);

				if cpu.flags & CPU_FLAG_ENABLED > 0 {
					if cpu.flags & CPU_FLAG_BOOT_PROCESSOR > 0 {
						// When HermitCore first boots up, core_id is initialized to 0.
						// For each application processor, it is later initialized with its Local APIC ID.
						// Consequently, the Local APIC ID for the boot processor must be 0 as well, or we will
						// run into inconsistencies when addressing CPUs in IPIs.
						assert!(cpu.local_apic_id == 0, "The Boot Processor has Local APIC ID {}. This is not supported!", cpu.local_apic_id);
					}

					local_apic_ids.push(cpu.local_apic_id);
				}

				current_address += mem::size_of::<CpuEntry>();
			},
			&1 => {
				// Bus
				let bus = unsafe { & *(current_address as *const BusEntry) };
				debug!("Found Bus entry: {}", bus);

				current_address += mem::size_of::<BusEntry>();
			},
			&2 => {
				// I/O APIC
				let ioapic = unsafe { & *(current_address as *const IoApicEntry) };
				debug!("Found I/O APIC entry: {}", ioapic);

				unsafe {
					IOAPIC_ADDRESS = virtualmem::allocate(BasePageSize::SIZE);
					debug!("Mapping IOAPIC at {:#X} to virtual address {:#X}", ioapic.address, IOAPIC_ADDRESS);

					paging::map::<BasePageSize>(
						IOAPIC_ADDRESS,
						ioapic.address as usize,
						1,
						PageTableEntryFlags::WRITABLE | PageTableEntryFlags::CACHE_DISABLE | PageTableEntryFlags::EXECUTE_DISABLE,
						false
					);
				}

				current_address += mem::size_of::<IoApicEntry>();
			},
			&3 => {
				// I/O Interrupt Assignment
				let io_interrupt = unsafe { & *(current_address as *const IoInterruptEntry) };
				debug!("Found I/O Interrupt entry: {}", io_interrupt);

				current_address += mem::size_of::<IoInterruptEntry>();
			},
			&4 => {
				// Local Interrupt Assignment
				let local_interrupt = unsafe { & *(current_address as *const LocalInterruptEntry) };
				debug!("Found Local Interrupt entry: {}", local_interrupt);

				current_address += mem::size_of::<LocalInterruptEntry>();
			},
			_ => {
				panic!("MultiProcessor Configuration Table contains invalid entry of type {}", entry_type);
			}
		}
	}

	// Successfully derived all information from the MultiProcessor tables.
	// Return the physical address of the Local APIC.
	Ok(mp_config_header.local_apic_address as usize)
}

fn detect_from_uhyve() -> Result<usize, ()> {
	if environment::is_uhyve() {
		return Ok(0xFEE00000 as usize);
	}

	Err(())
}

#[no_mangle]
pub extern "C" fn eoi() {
	local_apic_write(IA32_X2APIC_EOI, APIC_EOI_ACK);
}

pub fn init() {
	// Detect CPUs and APICs from the MultiProcessor Configuration Table (according to Intel MultiProcessor Specification 1.4).
	// ACPI is currently not supported.
	let local_apic_physical_address = detect_from_uhyve()
		.or_else(|_e| detect_from_multiprocessor_specification())
		.expect("HermitCore requires a MultiProcessor Specification 1.4 compliant system");

	// Initialize x2APIC or xAPIC, depending on what's available.
	init_x2apic();
	if !processor::supports_x2apic() {
		// We use the traditional xAPIC mode available on all x86-64 CPUs.
		// It uses a mapped page for communication.
		unsafe {
			LOCAL_APIC_ADDRESS = virtualmem::allocate(BasePageSize::SIZE);
			debug!("Mapping Local APIC at {:#X} to virtual address {:#X}", local_apic_physical_address, LOCAL_APIC_ADDRESS);

			paging::map::<BasePageSize>(
				LOCAL_APIC_ADDRESS,
				local_apic_physical_address,
				1,
				PageTableEntryFlags::WRITABLE | PageTableEntryFlags::CACHE_DISABLE | PageTableEntryFlags::EXECUTE_DISABLE,
				false
			);
		}
	}

	// Set gates to ISRs for the APIC interrupts we are going to enable.
	idt::set_gate(TLB_FLUSH_INTERRUPT_NUMBER, tlb_flush_handler as usize, 1);
	idt::set_gate(ERROR_INTERRUPT_NUMBER, error_interrupt_handler as usize, 1);
	idt::set_gate(SPURIOUS_INTERRUPT_NUMBER, spurious_interrupt_handler as usize, 1);
	idt::set_gate(WAKEUP_INTERRUPT_NUMBER, wakeup_handler as usize, 1);

	// Initialize interrupt handling over APIC.
	// All interrupts of the PIC have already been masked, so it doesn't need to be disabled again.
	init_local_apic();

	// Calibrate the APIC Timer once and use the calibration value for all CPUs.
	calibrate_timer();

	// init ioapic
	if !environment::is_uhyve() {
		init_ioapic();
	}
}

fn init_ioapic() {
	let max_entry = ioapic_max_redirection_entry()+1;
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

fn ioapic_inton(irq: u8, apicid: u8) -> Result<(), ()>
{
	if irq > 24 {
		error!("IOAPIC: trying to turn on irq {} which is too high\n", irq);
		return Err(());
	}

	let off = (irq*2) as u32;
	let ioredirect_upper: u32 = (apicid as u32) << 24;
	let ioredirect_lower: u32 = (0x20+irq) as u32;

	ioapic_write(IOAPIC_REG_TABLE+off, ioredirect_lower);
	ioapic_write(IOAPIC_REG_TABLE+1+off, ioredirect_upper);

	Ok(())
}

fn ioapic_intoff(irq: u32, apicid: u32) -> Result<(), ()>
{
	if irq > 24 {
		error!("IOAPIC: trying to turn off irq {} which is too high\n", irq);
		return Err(());
	}

	let off = (irq*2) as u32;
	let ioredirect_upper: u32 = (apicid as u32) << 24;
	let ioredirect_lower: u32 = ((0x20+irq) as u32) | (1 << 16); // turn it off (start masking)

	ioapic_write(IOAPIC_REG_TABLE+off, ioredirect_lower);
	ioapic_write(IOAPIC_REG_TABLE+1+off, ioredirect_upper);

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
	local_apic_write(IA32_X2APIC_LVT_ERROR, ERROR_INTERRUPT_NUMBER as u64);

	// allow all interrupts
	local_apic_write(IA32_X2APIC_TPR, 0x00);

	// Finally, enable the Local APIC by setting the interrupt number for spurious interrupts
	// and providing the enable bit.
	local_apic_write(IA32_X2APIC_SIVR, APIC_SIVR_ENABLED | (SPURIOUS_INTERRUPT_NUMBER as u64));
}

fn calibrate_timer() {
	// The APIC Timer is used to provide a one-shot interrupt for the tickless timer
	// implemented through processor::update_timer_ticks.
	// Therefore calibrate it relative to processor::TIMER_FREQUENCY, count 3 ticks here for accuracy.
	let tick_count = 3;
	let cycles_per_tick = processor::get_frequency() as u64 * 1_000_000 / processor::TIMER_FREQUENCY as u64;
	let cycles = tick_count * cycles_per_tick;

	// Disable interrupts for calibration accuracy and initialize the counter.
	// Dividing by the maximum value of 128 still provides enough accuracy for later setting timeouts in the range
	// of milliseconds, but especially allows for long timeouts.
	irq::disable();
	local_apic_write(IA32_X2APIC_DIV_CONF, APIC_DIV_CONF_DIVIDE_BY_128);
	local_apic_write(IA32_X2APIC_INIT_COUNT, u32::MAX as u64);

	// Wait until the 3 ticks have elapsed.
	let end = processor::get_timestamp() + cycles;
	while processor::get_timestamp() < end {
		spin_loop_hint();
	}

	// Save the difference of the initial value and current value as the result of the calibration
	// and reenable interrupts.
	unsafe {
		CALIBRATED_COUNTER_VALUE = ((u32::MAX - local_apic_read(IA32_X2APIC_CUR_COUNT)) / tick_count as u32) as usize;
		debug!(
			"Calibrated APIC Timer with a counter value of {} for a single tick of a {} Hz timer",
			CALIBRATED_COUNTER_VALUE,
			processor::TIMER_FREQUENCY
		);
	}
	irq::enable();
}

pub fn set_oneshot_timer(wakeup_time: Option<usize>) {
	if let Some(wt) = wakeup_time {
		// Calculate the relative timeout from the absolute wakeup time.
		// Maintain a minimum value of one tick, otherwise the timer interrupt does not fire at all.
		let current_time = processor::update_timer_ticks();
		let ticks = if wt > current_time { wt - current_time } else { 1 };

		// Enable the APIC Timer and let it start by setting the initial counter value.
		local_apic_write(IA32_X2APIC_LVT_TIMER, TIMER_INTERRUPT_NUMBER as u64);
		local_apic_write(IA32_X2APIC_INIT_COUNT, (unsafe { CALIBRATED_COUNTER_VALUE } * ticks) as u64);
	} else {
		// Disable the APIC Timer.
		local_apic_write(IA32_X2APIC_LVT_TIMER, APIC_LVT_MASK);
	}
}

pub fn init_x2apic() {
	if processor::supports_x2apic() {
		// The CPU supports the modern x2APIC mode, which uses MSRs for communication.
		// Enable it.
		let mut apic_base = unsafe { rdmsr(IA32_APIC_BASE) };
		apic_base |= X2APIC_ENABLE;
		unsafe { wrmsr(IA32_APIC_BASE, apic_base); }
	}
}

/// Boot all Application Processors
/// This algorithm is derived from Intel MultiProcessor Specification 1.4, B.4, but testing has shown
/// that a second STARTUP IPI and setting the BIOS Reset Vector are no longer necessary.
/// This is partly confirmed by https://wiki.osdev.org/Symmetric_Multiprocessing
pub fn boot_application_processors() {
	// We shouldn't have any problems fitting the boot code into a single page, but let's better be sure.
	assert!(SMP_BOOT_CODE.len() < BasePageSize::SIZE, "SMP Boot Code is larger than a page");
	debug!("SMP boot code is {} bytes long", SMP_BOOT_CODE.len());

	// Identity-map the boot code page and copy over the code.
	debug!("Mapping SMP boot code to physical and virtual address {:#X}", SMP_BOOT_CODE_ADDRESS);
	paging::map::<BasePageSize>(SMP_BOOT_CODE_ADDRESS, SMP_BOOT_CODE_ADDRESS, 1, PageTableEntryFlags::WRITABLE, false);
	unsafe { ptr::copy_nonoverlapping(&SMP_BOOT_CODE as *const u8, SMP_BOOT_CODE_ADDRESS as *mut u8, SMP_BOOT_CODE.len()); }

	// Pass the PML4 page table address to the boot code.
	unsafe { *((SMP_BOOT_CODE_ADDRESS + SMP_BOOT_CODE_OFFSET_PML4) as *mut u32) = cr3() as u32; }

	// Now wake up each application processor.
	let core_id = core_id() as u8;

	for apic_id in unsafe { CPU_LOCAL_APIC_IDS.as_ref().unwrap().iter() } {
		if *apic_id != core_id {
			let destination = (*apic_id as u64) << 32;
			debug!("Waking up CPU with Local APIC ID {}", *apic_id);

			// Allocate stack and PerCoreVariables structure for the CPU and pass the addresses.
			// Keep the stack executable to possibly support dynamically generated code on the stack (see https://security.stackexchange.com/a/47825).
			let stack = mm::allocate(KERNEL_STACK_SIZE, PageTableEntryFlags::empty());
			let boxed_percore = Box::new(PerCoreVariables::new(*apic_id as u32));
			unsafe {
				ptr::write_volatile(&mut current_stack_address, stack);
				ptr::write_volatile(&mut current_percore_address, Box::into_raw(boxed_percore) as usize);
			}

			// Save the current number of initialized CPUs.
			let current_cpu_online = unsafe { ptr::read_volatile(&cpu_online) };

			// Send an INIT IPI.
			local_apic_write(IA32_X2APIC_ICR, destination | APIC_ICR_LEVEL_TRIGGERED | APIC_ICR_LEVEL_ASSERT | APIC_ICR_DELIVERY_MODE_INIT);
			processor::udelay(200);

			local_apic_write(IA32_X2APIC_ICR, destination | APIC_ICR_LEVEL_TRIGGERED | APIC_ICR_DELIVERY_MODE_INIT);
			processor::udelay(10000);

			// Send a STARTUP IPI.
			local_apic_write(IA32_X2APIC_ICR, destination | APIC_ICR_DELIVERY_MODE_STARTUP | ((SMP_BOOT_CODE_ADDRESS as u64) >> 12));
			debug!("Waiting for it to respond");

			// Wait until the application processor has finished initializing.
			// It will indicate this by counting up cpu_online.
			while current_cpu_online == unsafe { ptr::read_volatile(&cpu_online) } {
				processor::udelay(1000);
			}
		}
	}
}

pub fn ipi_tlb_flush() {
	if unsafe { ptr::read_volatile(&cpu_online) } > 1 {
		let core_id = core_id() as u8;

		// Ensure that all memory operations have completed before issuing a TLB flush.
		unsafe { asm!("mfence" ::: "memory" : "volatile"); }

		// Send an IPI with our TLB Flush interrupt number to all other CPUs.
		for apic_id in unsafe { CPU_LOCAL_APIC_IDS.as_ref().unwrap().iter() } {
			if *apic_id != core_id {
				let destination = (*apic_id as u64) << 32;
				local_apic_write(IA32_X2APIC_ICR, destination | APIC_ICR_LEVEL_ASSERT | APIC_ICR_DELIVERY_MODE_FIXED | (TLB_FLUSH_INTERRUPT_NUMBER as u64));
			}
		}
	}
}

/// Gets the Core ID (here Local APIC ID) for a given sequential CPU number.
/// Both numbers often match, but don't need to (e.g. when a core has been disabled).
#[inline]
pub fn get_core_id_for_cpu_number(cpu_number: usize) -> Option<u32> {
	let apic_ids = unsafe { CPU_LOCAL_APIC_IDS.as_ref().unwrap() };
	if cpu_number < apic_ids.len() {
		Some(apic_ids[cpu_number] as u32)
	} else {
		None
	}
}

/// Send an inter-processor interrupt to wake up a CPU Core that is in a HALT state.
pub fn wakeup_core(core_to_wakeup: u32) {
	if core_to_wakeup != core_id() {
		let destination = (core_to_wakeup as u64) << 32;
		local_apic_write(IA32_X2APIC_ICR, destination | APIC_ICR_LEVEL_ASSERT | APIC_ICR_DELIVERY_MODE_FIXED | (WAKEUP_INTERRUPT_NUMBER as u64));
	}
}

/// Translate the x2APIC MSR into an xAPIC memory address.
#[inline]
fn translate_x2apic_msr_to_xapic_address(x2apic_msr: u32) -> usize {
	unsafe { LOCAL_APIC_ADDRESS + ((x2apic_msr as usize & 0xFF) << 4) }
}

fn local_apic_read(x2apic_msr: u32) -> u32 {
	if processor::supports_x2apic() {
		// x2APIC is simple, we can just read from the given MSR.
		unsafe { rdmsr(x2apic_msr) as u32 }
	} else {
		unsafe { *(translate_x2apic_msr_to_xapic_address(x2apic_msr) as *const u32) }
	}
}

fn ioapic_write(reg: u32, value: u32)
{
	unsafe {
		ptr::write_volatile(IOAPIC_ADDRESS as *mut u32, reg);
		ptr::write_volatile((IOAPIC_ADDRESS + 4*mem::size_of::<u32>()) as *mut u32, value);
	}
}

fn ioapic_read(reg: u32) -> u32
{
	let value;

	unsafe {
		ptr::write_volatile(IOAPIC_ADDRESS as *mut u32, reg);
		value = ptr::read_volatile((IOAPIC_ADDRESS + 4*mem::size_of::<u32>()) as *const u32);
	}

	value
}

fn ioapic_version() -> u32
{
	ioapic_read(IOAPIC_REG_VER) & 0xFF
}

fn ioapic_max_redirection_entry() -> u8
{
	((ioapic_read(IOAPIC_REG_VER) >> 16) & 0xFF) as u8
}

fn local_apic_write(x2apic_msr: u32, value: u64) {
	if processor::supports_x2apic() {
		// x2APIC is simple, we can just write the given value to the given MSR.
		unsafe { wrmsr(x2apic_msr, value); }
	} else {
		if x2apic_msr == IA32_X2APIC_ICR {
			// Instead of a single 64-bit ICR register, xAPIC has two 32-bit registers (ICR1 and ICR2).
			// There is a gap between them and the destination field in ICR2 is also 8 bits instead of 32 bits.
			let destination = ((value >> 8) & 0xFF00_0000) as u32;
			let icr2 = unsafe { &mut *((LOCAL_APIC_ADDRESS + APIC_ICR2) as *mut u32) };
			*icr2 = destination;

			// The remaining data without the destination will now be written into ICR1.
		}

		// Write the value.
		let value_ref = unsafe { &mut *(translate_x2apic_msr_to_xapic_address(x2apic_msr) as *mut u32) };
		*value_ref = value as u32;

		if x2apic_msr == IA32_X2APIC_ICR {
			// The ICR1 register in xAPIC mode also has a Delivery Status bit that must be checked.
			// Wait until the CPU clears it.
			// This bit does not exist in x2APIC mode (cf. Intel Vol. 3A, 10.12.9).
			while (unsafe { ptr::read_volatile(value_ref) } & APIC_ICR_DELIVERY_STATUS_PENDING) > 0 {
				spin_loop_hint();
			}
		}
	}
}

pub fn print_information() {
	infoheader!(" MULTIPROCESSOR INFORMATION ");
	infoentry!("APIC in use", if processor::supports_x2apic() { "x2APIC" } else { "xAPIC" });
	infoentry!("Initialized CPUs", unsafe { ptr::read_volatile(&cpu_online) });
	infofooter!();
}
