use core::{ptr, slice, str};

use align_address::Align;
use memory_addresses::{PhysAddr, VirtAddr};
use x86_64::structures::paging::{PageTableFlags, PhysFrame};

use crate::arch::mm::paging;
use crate::arch::mm::paging::{BasePageSize, LargePageSize, PageSize};
use crate::env::{self, StartInfo};

/// Memory at this physical address is supposed to contain a pointer to the Extended BIOS Data Area (EBDA).
const EBDA_PTR_LOCATION: PhysAddr = PhysAddr::new(0x0000_040e);
/// Minimum physical address where a valid EBDA must be located.
const EBDA_MINIMUM_ADDRESS: PhysAddr = PhysAddr::new(0x400);
/// The size of the EBDA window that is searched for an ACPI RSDP.
const EBDA_WINDOW_SIZE: usize = 1024;
/// The lower bound of the other address range, where the ACPI RSDP could be located.
const RSDP_SEARCH_ADDRESS_LOW: PhysAddr = PhysAddr::new(0xe_0000);
/// The upper bound of the other address range, where the ACPI RSDP could be located.
const RSDP_SEARCH_ADDRESS_HIGH: PhysAddr = PhysAddr::new(0xf_ffff);
/// Length in bytes of the structure, over which the basic (ACPI 1.0) checksum is calculated.
const RSDP_CHECKSUM_LENGTH: usize = 20;
/// Length in byte sof the structure, over which the extended (ACPI 2.0+) checksum is calculated.
const RSDP_XCHECKSUM_LENGTH: usize = 36;

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
#[derive(Clone, Copy, Debug)]
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
#[derive(Debug)]
struct AcpiTable<'a> {
	header: &'a AcpiSdtHeader,
}

impl AcpiTable<'_> {
	fn map(phys_addr: PhysAddr) -> Self {
		// Allocate at least two consecutive pages to ensure the `length` field is always readable, even when it is on the next page.
		let page_count = 2;
		let frame_start_addr = phys_addr.align_down(LargePageSize::SIZE);

		for i in 0..page_count {
			let virt_addr = VirtAddr::new(frame_start_addr.as_u64()) + i * LargePageSize::SIZE;
			let phys_addr = paging::virtual_to_physical(virt_addr);
			let expected_phys_addr = PhysAddr::new(virt_addr.as_u64());

			// Does not use `paging::identity_map()` since this mapping should not be `WRITABLE` and be `NO_EXECUTE`.
			if phys_addr != Some(expected_phys_addr) {
				paging::map::<LargePageSize>(
					virt_addr,
					expected_phys_addr,
					1,
					PageTableFlags::NO_EXECUTE,
				);
			}
		}

		let header_ptr = ptr::with_exposed_provenance::<AcpiSdtHeader>(phys_addr.as_usize());
		let table_length = u64::from(unsafe { (*header_ptr).length });
		assert!(phys_addr + table_length <= frame_start_addr + page_count * LargePageSize::SIZE);

		Self {
			header: unsafe { &*header_ptr },
		}
	}

	fn header_start_address(&self) -> usize {
		ptr::from_ref(self.header).addr()
	}

	fn table_start_address(&self) -> usize {
		self.header_start_address() + size_of::<AcpiSdtHeader>()
	}

	fn table_end_address(&self) -> usize {
		self.header_start_address() + self.header.length as usize
	}
}

/// Verifies the checksum of an ACPI table.
/// Tables supporting this feature contain a "checksum" field. The value of this field is chosen, so that a
/// (wrapping) sum over all table fields equals zero.
fn verify_checksum(start_address: usize, length: usize) -> Result<(), ()> {
	// Get a slice over all bytes of the structure that are considered for the checksum.
	let slice =
		unsafe { slice::from_raw_parts(ptr::with_exposed_provenance(start_address), length) };

	// Perform a wrapping sum over these bytes.
	let checksum = slice.iter().fold(0, |acc: u8, x| acc.wrapping_add(*x));

	// This sum must equal to zero to be valid.
	if checksum == 0 { Ok(()) } else { Err(()) }
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
			let frame = PhysFrame::<BasePageSize>::containing_address(x86_64::PhysAddr::new(
				current_address as u64,
			));
			paging::identity_map::<BasePageSize>(frame.start_address().into());
			current_page = current_address / BasePageSize::SIZE as usize;
		}

		// Verify the signature to find out if this is really an ACPI RSDP.
		let rsdp = unsafe { &*(ptr::with_exposed_provenance::<AcpiRsdp>(current_address)) };
		if &rsdp.signature != b"RSD PTR " {
			continue;
		}

		// Verify the basic checksum.
		if verify_checksum(current_address, RSDP_CHECKSUM_LENGTH).is_err() {
			debug!("Found an ACPI table at {current_address:#X}, but its RSDP checksum is invalid");
			continue;
		}

		// Verify the extended checksum if this is an ACPI 2.0-compliant table.
		if rsdp.revision >= 2 && verify_checksum(current_address, RSDP_XCHECKSUM_LENGTH).is_err() {
			debug!(
				"Found an ACPI table at {current_address:#X}, but its RSDP extended checksum is invalid"
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
	if let Some(rsdp_addr) = env::start_info().rsdp_addr() {
		trace!("RSDP detected successfully at {rsdp_addr:#x?}");
		let rsdp = unsafe {
			ptr::with_exposed_provenance::<AcpiRsdp>(rsdp_addr.get())
				.as_ref()
				.unwrap()
		};
		assert!(&rsdp.signature == b"RSD PTR ", "RSDP Address not valid!");
		return Ok(rsdp);
	}

	// Get the address of the EBDA.
	let frame = PhysFrame::<BasePageSize>::containing_address(EBDA_PTR_LOCATION.into());
	paging::identity_map::<BasePageSize>(frame.start_address().into());
	let ebda_ptr_location: &u16 =
		unsafe { &*(VirtAddr::from(EBDA_PTR_LOCATION.as_u64()).as_ptr()) };
	let ebda_address = PhysAddr::new(u64::from(*ebda_ptr_location) << 4);

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

pub fn init() {
	#[cfg(feature = "uhyve")]
	use env::UhyveStartInfo;

	#[cfg(feature = "uhyve")]
	if env::start_info().is_uhyve() {
		return;
	}

	// Detect the RSDP and get a pointer to either the XSDT (64-bit) or RSDT (32-bit), whichever is available.
	// Both are called RSDT in the following.
	let rsdp = detect_acpi().expect("Hermit requires an ACPI-compliant system");
	let rsdt_physical_address = if rsdp.revision >= 2 {
		PhysAddr::new(rsdp.xsdt_physical_address)
	} else {
		PhysAddr::new(rsdp.rsdt_physical_address.into())
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
			let address = unsafe {
				PhysAddr::new(ptr::with_exposed_provenance::<u64>(current_address).read_unaligned())
			};
			current_address += size_of::<u64>();
			address
		} else {
			let address = unsafe {
				PhysAddr::new(
					ptr::with_exposed_provenance::<u32>(current_address)
						.read_unaligned()
						.into(),
				)
			};
			current_address += size_of::<u32>();
			address
		};

		let table = AcpiTable::map(table_physical_address);
		debug!("Found ACPI table: {}", table.header.signature());
	}
}
