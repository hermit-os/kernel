// Copyright (c) 2018 Colin Finck, RWTH Aachen University
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

pub mod paging;
pub mod processor;
pub mod serial;

use arch::aarch64::paging::{BasePageSize, LargePageSize, PageSize, PageTableEntryFlags};
use arch::aarch64::serial::SerialPort;
use byteorder::{BigEndian, ByteOrder};
use elf::*;
use hermit_dtb::Dtb;
use physicalmem;

extern "C" {
	static dtb_address: usize;
	static kernel_end: u8;
}

// CONSTANTS
pub const ELF_ARCH: u16 = ELF_EM_AARCH64;

const SERIAL_PORT_ADDRESS: u32 = 0x900_0000;
const SERIAL_PORT_BAUDRATE: u32 = 115200;

// VARIABLES
static COM1: SerialPort = SerialPort::new(SERIAL_PORT_ADDRESS);

// FUNCTIONS
pub fn message_output_init() {
	COM1.init(SERIAL_PORT_BAUDRATE);
}

pub fn output_message_byte(byte: u8) {
	COM1.write_byte(byte);
}

pub unsafe fn find_kernel() -> usize {
	let mut flags = PageTableEntryFlags::empty();
	flags.normal().read_only().execute_disable();

	// The loader is on an identity-mapped 2 MiB page.
	// Claim the memory after the loader as available physical memory.
	let physical_address = align_up!(&kernel_end as *const u8 as usize, LargePageSize::SIZE);
	physicalmem::init(physical_address);

	// Identity-map the Flattened Device Tree (FDT or DTB).
	// It's usually larger than 4 KiB, so take a single large page (2 MiB).
	assert!(dtb_address > 0, "Could not find a DTB");
	loaderlog!("Found DTB at {:#X}", dtb_address);
	let page_address = align_down!(dtb_address, BasePageSize::SIZE);
	paging::map::<LargePageSize>(page_address, page_address, 1, flags);

	// Parse the DTB and find information about the initrd.
	// TODO: Make this a lazy_static like MULTIBOOT for x86_64. We will need it in several functions!
	let dtb = Dtb::from_raw(dtb_address as *const u8).expect("DTB has invalid header");

	if let Some(data) = dtb.get_property("/chosen", "linux,initrd-start") {
		// Identity-map the ELF header of the initrd.
		let start_address = BigEndian::read_u32(data) as usize;
		assert!(start_address > 0);
		loaderlog!("Found an ELF module at {:#X}", start_address);
		let page_address = align_down!(start_address, BasePageSize::SIZE);
		paging::map::<BasePageSize>(page_address, page_address, 1, flags);

		// TODO: For some reason, accessing the mapped ELF header only works in emulated
		// qemu-system-aarch64, but not with -enable-kvm. A KVM-virtualized system just
		// hangs when accessing the ELF header in check_kernel_elf_file.
		//
		// This is somehow related to the last paging::map::<BasePageSize> call, which is
		// guaranteed to allocate a new page table from physical memory.
		// The problem does not occur if the TLB is totally flushed after allocating the new
		// page table ("tlbi vmalle1is; dsb ish"), but this is no reasonable solution.

		start_address
	} else {
		panic!("DTB does not contain a \"linux,initrd-start\" property in the /chosen node!");
	}
}

pub unsafe fn move_kernel(physical_address: usize, virtual_address: usize, file_size: usize) -> usize {
	// TODO: Two options here:
	//
	//   * Either move the kernel to the lowest possible address in physical memory and map
	//     the virtual address to that.
	//     The kernel can then go and take everything after kernel_end as available physical memory,
	//     just like on x86_64.
	//
	//   * Alternatively, keep the kernel where it is (QEMU usually puts it in the middle of the
	//     physical memory), assert that it's already on a LargePageSize boundary, and just map the
	//     virtual address.
	//     However, the kernel then needs to call physicalmem::reserve to not use the taken physical
	//     memory for allocations later.
	//     This is probably the better way to proceed!
	//
	loaderlog!("Phys: {:#X}", physical_address);
	loaderlog!("Virt: {:#X}", virtual_address);
	panic!("");
}

pub unsafe fn boot_kernel(new_physical_address: usize, virtual_address: usize, mem_size: usize, entry_point: usize) {
	// TODO: Supply parameters to HermitCore-rs.
	//
	//   * base = new_physical_address, image_size = mem_size
	//   * uart_mmio can stay on 0x900_0000 as hardcoded in HermitCore-rs' entry.S or be read
	//     from the "/pl011@" path in the DTB.
	//   * cmdline and cmdsize via the "bootargs" property of the "/chosen" node in the DTB.
	//   * Possibly a new parameter dtb_address to pass on the dtb_address to the kernel.
	//     It would also be cleaner on x86_64 to directly let the loader write into the mb_info
	//     entry.asm variable than passing it through the rdx register and let entry.asm pick it up.
	//
	// Finally, jump to the ELF entry point.
	// But don't forget to adjust HermitCore-rs' entry.S first. It currently disables MMU on startup,
	// which would crash right away. This part can simply be removed to leave the MMU in its current state.
	// Regarding the actual page tables, we again have two options:
	//
	//   * Either port the x86_64 entry.asm code to relocate page tables and remap the
	//     kernel based on the passed physical address.
	//     Don't forget to add the identity-mapping for QEMU's UART at 0x900_0000!
	//     We would start with clean page tables then and know what regions are used
	//     by the physical memory manager.
	//
	//   * Alternatively, detect the state of the MMU and reuse the loader's page tables if paging is already
	//     enabled. The ASM code would be simpler, however it's not trivial to find out what physical
	//     memory regions have been used by the loader for page tables.
	//     These would need to be reserved in the physical memory manager of HermitCore-rs.
}
