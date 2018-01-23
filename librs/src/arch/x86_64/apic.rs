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

include!(concat!(env!("CARGO_TARGET_DIR"), "/smp_boot_code.rs"));

use alloc::vec::Vec;
use arch::x86_64::idt;
use arch::x86_64::irq;
use arch::x86_64::mm::paging;
use arch::x86_64::mm::paging::{BasePageSize, PageSize, PageTableEntryFlags};
use arch::x86_64::percore::*;
use arch::x86_64::processor;
use consts::*;
use core::sync::atomic::hint_core_should_pause;
use core::{mem, ptr, str};
use mm;
use scheduler;
use x86::shared::control_regs::*;
use x86::shared::io::*;
use x86::shared::msr::*;


extern "C" {
	static cpu_online: u32;
	static current_boot_id: u32;
}

const APIC_ICR2: usize = 0x0310;

const APIC_EOI_ACK: u64                     = 0;
const APIC_ICR_DELIVERY_MODE_FIXED: u64     = 0x000;
const APIC_ICR_DELIVERY_MODE_INIT: u64      = 0x500;
const APIC_ICR_DELIVERY_MODE_STARTUP: u64   = 0x600;
const APIC_ICR_DELIVERY_STATUS_PENDING: u32 = 1 << 12;
const APIC_ICR_LEVEL_TRIGGERED: u64         = 1 << 15;
const APIC_ICR_LEVEL_ASSERT: u64            = 1 << 14;
const APIC_LVT_MASK: u64                    = 1 << 16;
const APIC_SIVR_ENABLED: u64                = 1 << 8;

const CMOS_ADDRESS_PORT: u16                         = 0x70;
const CMOS_DATA_PORT: u16                            = 0x71;
const CMOS_SHUTDOWN_STATUS: u8                       = 0x0F;
const CMOS_SHUTDOWN_STATUS_JMP_DWORD_WITHOUT_EOI: u8 = 0x0A;

/// Warm Reset Vector at CS=0x40, IP=0x67
const RESET_VECTOR_OFFSET: usize = 0x467;

const TLB_FLUSH_INTERRUPT_NUMBER: u8 = 112;
const ERROR_INTERRUPT_NUMBER: u8     = 126;
const SPURIOUS_INTERRUPT_NUMBER: u8  = 127;

/// Physical and virtual memory address for our SMP boot code.
///
/// While our boot processor is already in x86-64 mode, application processors boot up in 16-bit real mode
/// and need an address in the CS:IP addressing scheme to jump to.
/// The CS:IP addressing scheme is limited to 2^20 bytes (= 1 MiB).
const SMP_BOOT_CODE_ADDRESS: usize = 0x8000;

const X2APIC_ENABLE: u64 = 1 << 10;

static mut LOCAL_APIC_ADDRESS: usize = 0;

/// Stores the Local APIC IDs of all CPUs.
/// As Rust currently implements no way of zero-initializing a global Vec in a no_std environment, we have to encapsulate it in an Option...
static mut CPU_LOCAL_APIC_IDS: Option<Vec<u8>> = None;


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

#[derive(Debug)]
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

const CPU_FLAG_ENABLED: u8        = 1 << 0;
const CPU_FLAG_BOOT_PROCESSOR: u8 = 1 << 1;

#[derive(Debug)]
#[repr(C, packed)]
struct BusEntry {
	entry_type: u8,
	id: u8,
	type_string: [u8; 6],
}

#[derive(Debug)]
#[repr(C, packed)]
struct IoApicEntry {
	entry_type: u8,
	id: u8,
	version: u8,
	flags: u8,
	address: u32,
}

#[derive(Debug)]
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

#[derive(Debug)]
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


extern "x86-interrupt" fn tlb_flush_handler(_stack_frame: &mut irq::ExceptionStackFrame) {
	debug!("tlb_flush_handler");
	unsafe { cr3_write(cr3()); }
	eoi();
}

extern "x86-interrupt" fn error_interrupt_handler(stack_frame: &mut irq::ExceptionStackFrame) {
	error!("APIC LVT Error Interrupt: {:#?}", stack_frame);
	eoi();
	scheduler::abort();
}

extern "x86-interrupt" fn spurious_interrupt_handler(stack_frame: &mut irq::ExceptionStackFrame) {
	error!("Spurious Interrupt: {:#?}", stack_frame);
	eoi();
	scheduler::abort();
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
				debug!("Found CPU entry: {:?}", cpu);

				if cpu.flags & CPU_FLAG_ENABLED > 0 {
					assert!(local_apic_ids.len() < MAX_CORES, "MultiProcessor Configuration Table contains more than the maximum supported {} CPUs", MAX_CORES);

					if cpu.flags & CPU_FLAG_BOOT_PROCESSOR > 0 {
						// When HermitCore first boots up, current_boot_id is initialized with 0.
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
				debug!("Found Bus entry: {:?}", bus);

				current_address += mem::size_of::<BusEntry>();
			},
			&2 => {
				// I/O APIC
				let ioapic = unsafe { & *(current_address as *const IoApicEntry) };
				debug!("Found I/O APIC entry: {:?}", ioapic);

				current_address += mem::size_of::<IoApicEntry>();
			},
			&3 => {
				// I/O Interrupt Assignment
				let io_interrupt = unsafe { & *(current_address as *const IoInterruptEntry) };
				debug!("Found I/O Interrupt entry: {:?}", io_interrupt);

				current_address += mem::size_of::<IoInterruptEntry>();
			},
			&4 => {
				// Local Interrupt Assignment
				let local_interrupt = unsafe { & *(current_address as *const LocalInterruptEntry) };
				debug!("Found Local Interrupt entry: {:?}", local_interrupt);

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

pub fn eoi() {
	local_apic_write(IA32_X2APIC_EOI, APIC_EOI_ACK);
}

pub fn init() {
	// Detect CPUs and APICs from the MultiProcessor Configuration Table (according to Intel MultiProcessor Specification 1.4).
	// ACPI is currently not supported.
	let local_apic_physical_address = detect_from_multiprocessor_specification()
		.unwrap_or_else(|_e| panic!("HermitCore requires a MultiProcessor Specification 1.4 compliant system"));

	// Initialize x2APIC or xAPIC, depending on what's available.
	init_x2apic();
	if !processor::supports_x2apic() {
		// We use the traditional xAPIC mode available on all x86-64 CPUs.
		// It uses a mapped page for communication. Map this page just below the kernel.
		unsafe {
			LOCAL_APIC_ADDRESS = mm::kernel_start_address() - BasePageSize::SIZE;
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

	// Initialize interrupt handling over APIC.
	// All interrupts of the PIC have already been masked, so it doesn't need to be disabled again.
	init_local_apic();

	// Initialize additional application processors.
	init_application_processors();
}

pub fn init_local_apic() {
	// Mask out all interrupts we never need.
	local_apic_write(IA32_X2APIC_LVT_TIMER, APIC_LVT_MASK);
	local_apic_write(IA32_X2APIC_LVT_THERMAL, APIC_LVT_MASK);
	local_apic_write(IA32_X2APIC_LVT_PMI, APIC_LVT_MASK);
	local_apic_write(IA32_X2APIC_LVT_LINT0, APIC_LVT_MASK);
	local_apic_write(IA32_X2APIC_LVT_LINT1, APIC_LVT_MASK);

	// Set the interrupt number of the APIC LVT Error interrupt.
	local_apic_write(IA32_X2APIC_LVT_ERROR, ERROR_INTERRUPT_NUMBER as u64);

	// Finally, enable the Local APIC by setting the interrupt number for spurious interrupts
	// and providing the enable bit.
	local_apic_write(IA32_X2APIC_SIVR, APIC_SIVR_ENABLED | (SPURIOUS_INTERRUPT_NUMBER as u64));
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

/// Initialize all Application Processors as described in Intel MultiProcessor Specification 1.4, B.4.
/// We only run the procedure for xAPIC and x2APIC here. The older 82489DX APIC has never been available for x86-64.
fn init_application_processors() {
	// We shouldn't have any problems fitting the boot code into a single page, but let's better be sure.
	assert!(SMP_BOOT_CODE.len() < BasePageSize::SIZE, "SMP Boot Code is larger than a page");
	debug!("SMP boot code is {} bytes long", SMP_BOOT_CODE.len());

	// Identity-map the boot code page and copy over the code.
	debug!("Mapping SMP boot code to physical and virtual address {:#X}", SMP_BOOT_CODE_ADDRESS);
	paging::map::<BasePageSize>(SMP_BOOT_CODE_ADDRESS, SMP_BOOT_CODE_ADDRESS, 1, PageTableEntryFlags::WRITABLE, false);
	unsafe { ptr::copy_nonoverlapping(&SMP_BOOT_CODE as *const u8, SMP_BOOT_CODE_ADDRESS as *mut u8, SMP_BOOT_CODE.len()); }

	// Find the placeholder in the code and replace it by the PML4 page table address in CR3.
	for i in 0..SMP_BOOT_CODE.len() {
		let placeholder = unsafe { &mut *((SMP_BOOT_CODE_ADDRESS + i) as *mut u32) };
		if *placeholder == 0xDEADBEAF {
			*placeholder = unsafe { cr3() as u32 };
			break;
		}
	}

	// Initialize the CMOS Shutdown Status Byte to let the Application Processor perform a JMP without EOI.
	unsafe {
		outb(CMOS_ADDRESS_PORT, CMOS_SHUTDOWN_STATUS);
		outb(CMOS_DATA_PORT, CMOS_SHUTDOWN_STATUS_JMP_DWORD_WITHOUT_EOI);
	}

	// Get us another virtual address just below the kernel to map the reset vector.
	// Then put in the CS:IP address of the boot code.
	let reset_vector_address = unsafe { LOCAL_APIC_ADDRESS - BasePageSize::SIZE };
	paging::map::<BasePageSize>(reset_vector_address, 0, 1, PageTableEntryFlags::WRITABLE | PageTableEntryFlags::CACHE_DISABLE, false);
	let reset_vector = unsafe { &mut *((reset_vector_address + RESET_VECTOR_OFFSET) as *mut u32) };
	*reset_vector = (SMP_BOOT_CODE_ADDRESS << 12) as u32;

	// Now wake up each application processor.
	let core_id = core_id() as u8;

	for apic_id in unsafe { CPU_LOCAL_APIC_IDS.as_ref().unwrap().iter() } {
		if *apic_id != core_id {
			let destination = (*apic_id as u64) << 32;
			debug!("Waking up CPU with Local APIC ID {}", *apic_id);

			// Save the current number of initialized CPUs.
			let current_cpu_online = unsafe { ptr::read_volatile(&cpu_online) };

			// Set the Local APIC ID for the next CPU we initialize.
			unsafe { current_boot_id.set_per_core(*apic_id as u32); }

			// Send an INIT IPI.
			local_apic_write(IA32_X2APIC_ICR, destination | APIC_ICR_LEVEL_TRIGGERED | APIC_ICR_LEVEL_ASSERT | APIC_ICR_DELIVERY_MODE_INIT);
			processor::udelay(200);

			local_apic_write(IA32_X2APIC_ICR, destination | APIC_ICR_LEVEL_TRIGGERED | APIC_ICR_DELIVERY_MODE_INIT);
			processor::udelay(10000);

			// Send two STARTUP IPIs and wait the 200usec as per the specification.
			for _i in 0..1 {
				local_apic_write(IA32_X2APIC_ICR, destination | APIC_ICR_DELIVERY_MODE_STARTUP | ((SMP_BOOT_CODE_ADDRESS as u64) >> 12));
				processor::udelay(200);
			}

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

fn local_apic_write(x2apic_msr: u32, value: u64) {
	if processor::supports_x2apic() {
		// x2APIC is simple, we can just write the given value to the given MSR.
		unsafe { wrmsr(x2apic_msr, value); }
	} else {
		// Translate the x2APIC register into an xAPIC memory address.
		let address = unsafe { LOCAL_APIC_ADDRESS } + ((x2apic_msr as usize & 0xFF) << 4);

		if x2apic_msr == IA32_X2APIC_ICR {
			// Instead of a single 64-bit ICR register, xAPIC has two 32-bit registers (ICR1 and ICR2).
			// There is a gap between them and the destination field in ICR2 is also 8 bits instead of 32 bits.
			let destination = ((value >> 8) & 0xFF00_0000) as u32;
			let icr2 = unsafe { &mut *((LOCAL_APIC_ADDRESS + APIC_ICR2) as *mut u32) };
			*icr2 = destination;

			// The remaining data without the destination will now be written into ICR1.
		}

		// Write the value.
		let value_ref = unsafe { &mut *(address as *mut u32) };
		*value_ref = value as u32;

		if x2apic_msr == IA32_X2APIC_ICR {
			// The ICR1 register in xAPIC mode also has a Delivery Status bit that must be checked.
			// Wait until the CPU clears it.
			// This bit does not exist in x2APIC mode (cf. Intel Vol. 3A, 10.12.9).
			while (unsafe { ptr::read_volatile(value_ref) } & APIC_ICR_DELIVERY_STATUS_PENDING) > 0 {
				hint_core_should_pause();
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
