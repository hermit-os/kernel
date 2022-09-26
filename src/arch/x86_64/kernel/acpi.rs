use crate::arch::x86_64::kernel::processor;
use crate::arch::x86_64::mm::paging::{
	BasePageSize, PageSize, PageTableEntryFlags, PageTableEntryFlagsExt,
};
use crate::arch::x86_64::mm::{paging, virtualmem};
use crate::arch::x86_64::mm::{PhysAddr, VirtAddr};
use crate::x86::io::*;
use core::convert::Infallible;
use core::{mem, ptr, slice, str};

/// Memory at this physical address is supposed to contain a pointer to the Extended BIOS Data Area (EBDA).
const EBDA_PTR_LOCATION: PhysAddr = PhysAddr(0x0000_040E);
/// Minimum physical address where a valid EBDA must be located.
const EBDA_MINIMUM_ADDRESS: PhysAddr = PhysAddr(0x400);
/// The size of the EBDA window that is searched for an ACPI RSDP.
const EBDA_WINDOW_SIZE: usize = 1024;
/// The lower bound of the other address range, where the ACPI RSDP could be located.
const RSDP_SEARCH_ADDRESS_LOW: PhysAddr = PhysAddr(0xE_0000);
/// The upper bound of the other address range, where the ACPI RSDP could be located.
const RSDP_SEARCH_ADDRESS_HIGH: PhysAddr = PhysAddr(0xF_FFFF);
/// Length in bytes of the structure, over which the basic (ACPI 1.0) checksum is calculated.
const RSDP_CHECKSUM_LENGTH: usize = 20;
/// Length in byte sof the structure, over which the extended (ACPI 2.0+) checksum is calculated.
const RSDP_XCHECKSUM_LENGTH: usize = 36;

/// ACPI AML opcode indicating that a name follows.
const AML_NAMEOP: u8 = 0x08;
/// ACPI AML opcode indicating that a package follows.
const AML_PACKAGEOP: u8 = 0x12;
/// ACPI AML opcode indicating a single zero byte as the data.
const AML_ZEROOP: u8 = 0x00;
/// ACPI AML opcode indicating a single one byte as the data.
const AML_ONEOP: u8 = 0x01;
/// ACPI AML opcode indicating that a single byte with the data follows.
const AML_BYTEPREFIX: u8 = 0x0A;

/// Bit to enable an ACPI Sleep State.
const SLP_EN: u16 = 1 << 13;

/// The "Multiple APIC Description Table" (MADT) preserved for get_apic_table().
static mut MADT: Option<AcpiTable<'_>> = None;
/// The PM1A Control I/O Port for powering off the computer through ACPI.
static mut PM1A_CNT_BLK: Option<u16> = None;
/// The Sleeping State Type code for powering off the computer through ACPI.
static mut SLP_TYPA: Option<u8> = None;

/// The "Root System Description Pointer" structure providing pointers to all other ACPI tables.
#[repr(C, packed)]
struct AcpiRsdp {
	signature: [u8; 8],
	checksum: u8,
	oem_id: [u8; 6],
	revision: u8,
	rsdt_physical_address: u32,
	length: u32,
	xsdt_physical_address: u64,
	extended_checksum: u8,
	reserved: [u8; 3],
}

impl AcpiRsdp {
	fn oem_id(&self) -> &str {
		str::from_utf8(&self.oem_id).unwrap()
	}
}

/// The header of (almost) every ACPI table.
#[repr(C, packed)]
struct AcpiSdtHeader {
	signature: [u8; 4],
	length: u32,
	revision: u8,
	checksum: u8,
	oem_id: [u8; 6],
	oem_table_id: [u8; 8],
	oem_revision: u32,
	creator_id: u32,
	creator_revision: u32,
}

impl AcpiSdtHeader {
	fn signature(&self) -> &str {
		str::from_utf8(&self.signature).unwrap()
	}
}

/// A convenience structure to work with an ACPI table.
/// Maps a single table to memory and frees the memory when a variable of this structure goes out of scope.
pub struct AcpiTable<'a> {
	header: &'a AcpiSdtHeader,
	allocated_virtual_address: VirtAddr,
	allocated_length: usize,
}

impl<'a> AcpiTable<'a> {
	fn map(physical_address: PhysAddr) -> Self {
		let mut flags = PageTableEntryFlags::empty();
		flags.normal().read_only().execute_disable();

		// Allocate two 4 KiB pages for the table and map it.
		// This guarantees that we can access at least the "length" field of the table header when its physical address
		// crosses a page boundary.
		let mut allocated_length = 2 * BasePageSize::SIZE as usize;
		let mut count = allocated_length / BasePageSize::SIZE as usize;

		let physical_map_address = physical_address.align_down_to_base_page();
		let offset: usize = (physical_address - physical_map_address).into();
		let mut virtual_address = virtualmem::allocate(allocated_length).unwrap();
		paging::map::<BasePageSize>(virtual_address, physical_map_address, count, flags);

		// Get a pointer to the header and query the table length.
		let mut header_ptr: *const AcpiSdtHeader = (virtual_address + offset).as_ptr();
		let table_length = unsafe { (*header_ptr).length } as usize;

		// Remap if the length exceeds what we've allocated.
		if table_length > allocated_length - offset {
			virtualmem::deallocate(virtual_address, allocated_length);

			allocated_length = align_up!(table_length + offset, BasePageSize::SIZE as usize);
			count = allocated_length / BasePageSize::SIZE as usize;

			virtual_address = virtualmem::allocate(allocated_length).unwrap();
			paging::map::<BasePageSize>(virtual_address, physical_map_address, count, flags);

			header_ptr = (virtual_address + offset).as_ptr();
		}

		// Return the table.
		Self {
			header: unsafe { &*header_ptr },
			allocated_virtual_address: virtual_address,
			allocated_length,
		}
	}

	pub fn header_start_address(&self) -> usize {
		self.header as *const _ as usize
	}

	pub fn table_start_address(&self) -> usize {
		self.header_start_address() + mem::size_of::<AcpiSdtHeader>()
	}

	pub fn table_end_address(&self) -> usize {
		self.header_start_address() + self.header.length as usize
	}
}

impl<'a> Drop for AcpiTable<'a> {
	fn drop(&mut self) {
		virtualmem::deallocate(self.allocated_virtual_address, self.allocated_length);
	}
}

/// The ACPI Generic Address Structure (GAS).
/// Described in ACPI Specification 6.2 A, 5.2.3.2 Generic Address Structure.
#[repr(C, packed)]
struct AcpiGenericAddress {
	address_space: u8,
	bit_width: u8,
	bit_offset: u8,
	access_size: u8,
	address: u64,
}

const GENERIC_ADDRESS_IO_SPACE: u8 = 1;

/// The "Fixed ACPI Description Table" (FADT), also called "Fixed ACPI Control Pointer" (FACP).
/// Described in ACPI Specification 6.2 A, 5.2.9 Fixed ACPI Description Table (FADT).
#[repr(C, packed)]
struct AcpiFadt {
	firmware_ctrl: u32,
	dsdt: u32,
	reserved1: u8,
	preferred_pm_profile: u8,
	sci_int: u16,
	smi_cmd: u32,
	acpi_enable: u8,
	acpi_disable: u8,
	s4bios_req: u8,
	pstate_cnt: u8,
	pm1a_evt_blk: u32,
	pm1b_evt_blk: u32,
	pm1a_cnt_blk: u32,
	pm1b_cnt_blk: u32,
	pm2_cnt_blk: u32,
	pm_tmr_blk: u32,
	gpe0_blk: u32,
	gpe1_blk: u32,
	pm1_evt_len: u8,
	pm1_cnt_len: u8,
	pm2_cnt_len: u8,
	pm_tmr_len: u8,
	gpe0_blk_len: u8,
	gpe1_blk_len: u8,
	gpe1_base: u8,
	cst_cnt: u8,
	p_lvl2_lat: u16,
	p_lvl3_lat: u16,
	flush_size: u16,
	flush_stride: u16,
	duty_offset: u8,
	duty_width: u8,
	day_alrm: u8,
	mon_alrm: u8,
	century: u8,
	iapc_boot_arch: u16,
	reserved2: u8,
	flags: u32,
	reset_reg: AcpiGenericAddress,
	reset_value: u8,
	arm_boot_arch: u16,
	fadt_minor_version: u8,
	x_firmware_ctrl: u64,
	x_dsdt: u64,
	x_pm1a_evt_blk: AcpiGenericAddress,
	x_pm1b_evt_blk: AcpiGenericAddress,
	x_pm1a_cnt_blk: AcpiGenericAddress,
	x_pm1b_cnt_blk: AcpiGenericAddress,
	x_pm2_cnt_blk: AcpiGenericAddress,
	x_pm_tmr_blk: AcpiGenericAddress,
	x_gpe0_blk: AcpiGenericAddress,
	x_gpe1_blk: AcpiGenericAddress,
	sleep_control_reg: AcpiGenericAddress,
	sleep_status_reg: AcpiGenericAddress,
	hypervisor_vendor_id: u64,
}

/// Verifies the checksum of an ACPI table.
/// Tables supporting this feature contain a "checksum" field. The value of this field is chosen, so that a
/// (wrapping) sum over all table fields equals zero.
fn verify_checksum(start_address: usize, length: usize) -> Result<(), ()> {
	// Get a slice over all bytes of the structure that are considered for the checksum.
	let slice = unsafe { slice::from_raw_parts(start_address as *const u8, length) };

	// Perform a wrapping sum over these bytes.
	let checksum = slice.iter().fold(0, |acc: u8, x| acc.wrapping_add(*x));

	// This sum must equal to zero to be valid.
	if checksum == 0 {
		Ok(())
	} else {
		Err(())
	}
}

/// Tries to find the ACPI RSDP within the specified address range.
/// Returns a reference to it within the Ok() if successful or an empty Err() on failure.
fn detect_rsdp(start_address: PhysAddr, end_address: PhysAddr) -> Result<&'static AcpiRsdp, ()> {
	// Trigger page mapping in the first iteration!
	let mut current_page = 0;

	// Look for the ACPI RSDP in all possible 16-byte aligned addresses within this range.
	for current_address in (start_address.as_usize()..end_address.as_usize()).step_by(16) {
		// Have we crossed a page boundary in the last iteration?
		if current_address / BasePageSize::SIZE as usize > current_page {
			// Identity-map this possible page of the RSDP.
			paging::identity_map(
				PhysAddr::from(current_address),
				PhysAddr::from(current_address),
			);
			current_page = current_address / BasePageSize::SIZE as usize;
		}

		// Verify the signature to find out if this is really an ACPI RSDP.
		let rsdp = unsafe { &*(current_address as *const AcpiRsdp) };
		if &rsdp.signature != b"RSD PTR " {
			continue;
		}

		// Verify the basic checksum.
		if verify_checksum(current_address, RSDP_CHECKSUM_LENGTH).is_err() {
			debug!(
				"Found an ACPI table at {:#X}, but its RSDP checksum is invalid",
				current_address
			);
			continue;
		}

		// Verify the extended checksum if this is an ACPI 2.0-compliant table.
		if rsdp.revision >= 2 && verify_checksum(current_address, RSDP_XCHECKSUM_LENGTH).is_err() {
			debug!(
				"Found an ACPI table at {:#X}, but its RSDP extended checksum is invalid",
				current_address
			);
			continue;
		}

		// We were successful! Return a pointer to the RSDT (whose 64-bit address is called XSDT in this structure).
		info!(
			"Found an ACPI revision {} table at {:#X} with OEM ID \"{}\"",
			rsdp.revision,
			current_address,
			rsdp.oem_id()
		);
		return Ok(rsdp);
	}

	// We found no valid ACPI RSDP.
	Err(())
}

/// Detects ACPI support of the computer system.
/// Returns a reference to the ACPI RSDP within the Ok() if successful or an empty Err() on failure.
fn detect_acpi() -> Result<&'static AcpiRsdp, ()> {
	// Get the address of the EBDA.
	paging::identity_map(EBDA_PTR_LOCATION, EBDA_PTR_LOCATION);
	let ebda_ptr_location: &u16 =
		unsafe { &*(VirtAddr::from(EBDA_PTR_LOCATION.as_u64()).as_ptr()) };
	let ebda_address = PhysAddr((*ebda_ptr_location as u64) << 4);

	// Check if the pointed address is valid. This check is also done in ACPICA.
	if ebda_address > EBDA_MINIMUM_ADDRESS {
		// Try to find an RSDP within the 1 KiB window of the EBDA.
		if let Ok(rsdp) = detect_rsdp(ebda_address, ebda_address + EBDA_WINDOW_SIZE) {
			return Ok(rsdp);
		}
	}

	// If we didn't find anything above, check the other memory range for an RSDP.
	if let Ok(rsdp) = detect_rsdp(RSDP_SEARCH_ADDRESS_LOW, RSDP_SEARCH_ADDRESS_HIGH) {
		return Ok(rsdp);
	}

	// We didn't find any ACPI tables.
	Err(())
}

fn search_s5_in_table(table: AcpiTable<'_>) {
	// Get the AML code.
	// As we do not implement an AML interpreter, we search through the bytecode.
	let aml = unsafe {
		slice::from_raw_parts(
			table.table_start_address() as *const u8,
			table.table_end_address() - table.table_start_address(),
		)
	};

	// Find the "_S5_" object in the bytecode.
	let s5 = [b'_', b'S', b'5', b'_', AML_PACKAGEOP];
	let s5_position = aml.windows(s5.len()).position(|window| window == s5);
	if let Some(i) = s5_position {
		// We have found an "_S5_" object that looks valid.
		// To be sure, verify that it begins with an AML_NAMEOP or an AML_NAMEOP and a backslash.
		if i > 2 && (aml[i - 1] == AML_NAMEOP || (aml[i - 2] == AML_NAMEOP && aml[i - 1] == b'\\'))
		{
			// This is a valid "_S5_" object.
			// It should be followed by this structure:
			//    - single byte for PkgLength (index 5)
			//    - single byte for NumElements (index 6)
			let pkg_length = aml[i + 5];
			let num_elements = aml[i + 6];

			// Bits 6-7 of PkgLength are non-zero for larger packages, resulting in a different structure.
			// This mustn't be the case for the "_S5_" object.
			if pkg_length & 0b1100_0000 == 0 && num_elements > 0 {
				// The next byte is an opcode describing the data.
				// It is usually the byte prefix, indicating that the actual data is the single byte following the opcode.
				// However, if the data is a zero or one byte, this may also be indicated by the opcode.
				let op = aml[i + 7];
				let slp_typa = match op {
					AML_ZEROOP => 0,
					AML_ONEOP => 1,
					AML_BYTEPREFIX => aml[i + 8],
					_ => return,
				};

				// All assumptions are correct, so slp_typa is supposed to contain valid information.
				// Now we have all information we need for powering off through ACPI.
				//
				// Note that Power Off may also be controlled through PM1B_CNT_BLK / SLP_TYPB
				// according to the ACPI Specification. However, this has not yet been observed on real computers
				// and therefore not implemented.
				unsafe {
					SLP_TYPA = Some(slp_typa);
				}
			}
		}
	}
}

fn parse_fadt(fadt: AcpiTable<'_>) {
	// Get us a reference to the actual fields of the FADT table.
	// Note that not all fields may be accessible depending on the ACPI revision of the computer.
	// Always check fadt.table_end_address() when accessing an optional field!
	let fadt_table = unsafe { &*(fadt.table_start_address() as *const AcpiFadt) };

	// Check if the FADT is large enough to hold an x_pm1a_cnt_blk field and if this field is non-zero.
	// In that case, it shall be preferred over the I/O port specified in pm1a_cnt_blk.
	// As all PM1 control registers are supposed to be in I/O space, we can simply check the address_space field
	// of x_pm1a_cnt_blk to determine the validity of x_pm1a_cnt_blk.
	let x_pm1a_cnt_blk_field_address = &fadt_table.x_pm1a_cnt_blk as *const _ as usize;
	let pm1a_cnt_blk = if x_pm1a_cnt_blk_field_address < fadt.table_end_address()
		&& fadt_table.x_pm1a_cnt_blk.address_space == GENERIC_ADDRESS_IO_SPACE
	{
		fadt_table.x_pm1a_cnt_blk.address as u16
	} else {
		fadt_table.pm1a_cnt_blk as u16
	};
	unsafe {
		PM1A_CNT_BLK = Some(pm1a_cnt_blk);
	}

	// Map the "Differentiated System Description Table" (DSDT).
	let x_dsdt_field_address = ptr::addr_of!(fadt_table.x_dsdt) as usize;
	let dsdt_address = if x_dsdt_field_address < fadt.table_end_address() && fadt_table.x_dsdt > 0 {
		PhysAddr(fadt_table.x_dsdt)
	} else {
		PhysAddr(fadt_table.dsdt.into())
	};
	let dsdt = AcpiTable::map(dsdt_address);

	// Check it.
	assert!(
		dsdt.header.signature() == "DSDT",
		"DSDT at {:#X} has invalid signature \"{}\"",
		dsdt_address,
		dsdt.header.signature()
	);
	assert!(
		verify_checksum(dsdt.header_start_address(), dsdt.header.length as usize).is_ok(),
		"DSDT at {:#X} has invalid checksum",
		dsdt_address
	);

	// Try to find the "_S5_" object for SLP_TYPA in the DSDT AML bytecode.
	// It may also be in an SSDT though.
	search_s5_in_table(dsdt);
}

fn parse_ssdt(ssdt: AcpiTable<'_>) {
	// We don't need to parse the SSDT if we already have information about the "_S5_" object
	// (e.g. from the DSDT or a previous SSDT).
	if unsafe { SLP_TYPA }.is_some() {
		return;
	}

	// Otherwise, just try to find "_S5_" information in the AML bytecode of this SSDT.
	search_s5_in_table(ssdt);
}

pub fn get_madt() -> Option<&'static AcpiTable<'static>> {
	unsafe { MADT.as_ref() }
}

pub fn poweroff() -> Result<Infallible, ()> {
	unsafe {
		if let (Some(pm1a_cnt_blk), Some(slp_typa)) = (PM1A_CNT_BLK, SLP_TYPA) {
			let bits = (u16::from(slp_typa) << 10) | SLP_EN;
			debug!(
				"Powering Off through ACPI (port {:#X}, bitmask {:#X})",
				pm1a_cnt_blk, bits
			);
			outw(pm1a_cnt_blk, bits);
			loop {
				processor::halt();
			}
		} else {
			warn!("ACPI Power Off is not available");
			Err(())
		}
	}
}

pub fn init() {
	// Detect the RSDP and get a pointer to either the XSDT (64-bit) or RSDT (32-bit), whichever is available.
	// Both are called RSDT in the following.
	let rsdp = detect_acpi().expect("HermitCore requires an ACPI-compliant system");
	let rsdt_physical_address = if rsdp.revision >= 2 {
		PhysAddr(rsdp.xsdt_physical_address)
	} else {
		PhysAddr(rsdp.rsdt_physical_address.into())
	};

	// Map the RSDT.
	let rsdt = AcpiTable::map(rsdt_physical_address);

	// The RSDT contains pointers to all available ACPI tables.
	// Iterate through them.
	let mut current_address = rsdt.table_start_address();
	while current_address < rsdt.table_end_address() {
		// Depending on the RSDP revision, either an XSDT or an RSDT has been chosen above.
		// The XSDT contains 64-bit pointers whereas the RSDT has 32-bit pointers.
		let table_physical_address = if rsdp.revision >= 2 {
			let address = PhysAddr(unsafe { *(current_address as *const u64) });
			current_address += mem::size_of::<u64>();
			address
		} else {
			let address = PhysAddr((unsafe { *(current_address as *const u32) }).into());
			current_address += mem::size_of::<u32>();
			address
		};

		let table = AcpiTable::map(table_physical_address);
		debug!("Found ACPI table: {}", table.header.signature());

		if table.header.signature() == "APIC" {
			// The "Multiple APIC Description Table" (MADT) aka "APIC Table" (APIC)
			// Check and save the entire APIC table for the get_apic_table() call.
			assert!(
				verify_checksum(table.header_start_address(), table.header.length as usize).is_ok(),
				"MADT at {:#X} has invalid checksum",
				table_physical_address
			);
			unsafe {
				MADT = Some(table);
			}
		} else if table.header.signature() == "FACP" {
			// The "Fixed ACPI Description Table" (FADT) aka "Fixed ACPI Control Pointer" (FACP)
			// Check and parse this table for the poweroff() call.
			assert!(
				verify_checksum(table.header_start_address(), table.header.length as usize).is_ok(),
				"FADT at {:#X} has invalid checksum",
				table_physical_address
			);
			parse_fadt(table);
		} else if table.header.signature() == "SSDT" {
			assert!(
				verify_checksum(table.header_start_address(), table.header.length as usize).is_ok(),
				"SSDT at {:#X} has invalid checksum",
				table_physical_address
			);
			parse_ssdt(table);
		}
	}
}
