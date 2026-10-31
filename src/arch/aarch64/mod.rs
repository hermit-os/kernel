mod console;

pub use self::console::Console;
pub mod drivers;
pub mod entry;
mod page_tables;
pub mod paging;

use core::arch::asm;
use core::ptr;

use aarch64_cpu::asm::barrier::{NSH, SY, dmb, dsb, isb};
use align_address::Align;
use fdt::Fdt;
use hermit_entry::Entry;
use hermit_entry::boot_info::{BootInfo, HardwareInfo, PlatformInfo, RawBootInfo, SerialPortBase};
use hermit_entry::elf::LoadedKernel;
use log::info;

use crate::arch::paging::*;
use crate::fdt_ext::FdtExt;
use crate::os::CONSOLE;
use crate::{BootInfoExt, stack};

/// start address of the RAM at Qemu's virt emulation
#[cfg(not(feature = "linux"))]
const RAM_START: u64 = 0x40000000;
/// start address of the RAM at Firecracker
#[cfg(feature = "linux")]
const RAM_START: u64 = 0x80000000;

/// Address of the device tree.
///
/// Qemu assumes for ELF kernel that the fdt is located at
/// start of RAM (0x4000_0000), see
/// <https://qemu.readthedocs.io/en/latest/system/arm/virt.html>.
/// The Linux boot protocol passes it in `x0` instead.
static mut DEVICE_TREE: u64 = RAM_START;

unsafe fn set_device_tree(addr: u64) {
	if cfg!(feature = "linux") {
		unsafe { ptr::write_volatile(&raw mut DEVICE_TREE, addr) }
	}
}

fn device_tree() -> u64 {
	unsafe { ptr::read_volatile(&raw const DEVICE_TREE) }
}

pub unsafe fn get_memory(_memory_size: u64) -> u64 {
	let loader_end = elf_symbols::executable_end();
	let loader_end = (loader_end.expose_provenance() as u64).align_up(LargePageSize::SIZE as u64);

	if cfg!(feature = "linux") {
		// The device tree lies at the end of RAM, which the page tables do
		// not cover, so it is moved behind the loader.
		let fdt = unsafe {
			Fdt::from_ptr(ptr::with_exposed_provenance(device_tree() as usize))
				.expect(".fdt file has invalid header")
		};
		let size = fdt.total_size();
		unsafe {
			ptr::copy_nonoverlapping(
				ptr::with_exposed_provenance::<u8>(device_tree() as usize),
				ptr::with_exposed_provenance_mut::<u8>(loader_end as usize),
				size,
			);
			set_device_tree(loader_end);
		}
		return (loader_end + size as u64).align_up(LargePageSize::SIZE as u64);
	}

	loader_end
}

pub fn find_kernel() -> &'static [u8] {
	let fdt = unsafe {
		Fdt::from_ptr(ptr::with_exposed_provenance(device_tree() as usize))
			.expect(".fdt file has invalid header")
	};

	fdt.find_kernel().unwrap()
}

#[allow(static_mut_refs)] // FIXME: disallow
pub unsafe fn boot_kernel(kernel_info: LoadedKernel) -> ! {
	let LoadedKernel {
		load_info,
		entry_point,
	} = kernel_info;

	#[cfg(feature = "linux")]
	unsafe {
		sync_icache(load_info.kernel_image_addr_range.clone());
	}

	let fdt = unsafe {
		Fdt::from_ptr(ptr::with_exposed_provenance(device_tree() as usize))
			.expect(".fdt file has invalid header")
	};
	let cpus = fdt.cpus().count();
	info!("Detect {cpus} CPU(s)");

	let uart_address: u32 = CONSOLE.lock().get().get_stdout();
	info!("Detect UART at {uart_address:#x}");

	unsafe {
		page_tables::init(uart_address);
	}

	CONSOLE.lock().get().set_stdout(0x1000);

	unsafe {
		page_tables::enable();
	}
	#[cfg(feature = "linux")]
	unsafe {
		ptr::write_volatile(&raw mut entry::EXCEPTION_UART, 0x1000);
	}

	let fdt = unsafe {
		Fdt::from_ptr(ptr::with_exposed_provenance(device_tree() as usize))
			.expect(".fdt file has invalid header")
	};

	if let Some(device_type) = fdt
		.find_node("/memory")
		.and_then(|node| node.property("device_type"))
	{
		let device_type = core::str::from_utf8(device_type.value)
			.unwrap()
			.trim_matches(char::from(0));
		assert!(device_type == "memory");
	}
	info!("Memory found!");
	let regions = fdt.memory().regions().next().unwrap();
	let ram_start = regions.starting_address as u64;
	let ram_size = regions.size.unwrap() as u64;

	info!("ram_start: {ram_start:#x}, ram_size: {ram_size:#x}. Trying to jump into kernel soon.");
	let boot_info = BootInfo {
		hardware_info: HardwareInfo {
			phys_addr_range: ram_start..ram_start + ram_size,
			serial_port_base: SerialPortBase::new(0x1000),
			device_tree: core::num::NonZeroU64::new(device_tree()),
		},
		load_info,
		platform_info: PlatformInfo::LinuxBoot,
	};

	let stack = stack::get_stack_ptr();
	let entry = ptr::with_exposed_provenance(entry_point.try_into().unwrap());
	let raw_boot_info = boot_info.write();

	unsafe { enter_kernel(stack, entry, raw_boot_info) }
}

/// Makes code written through the data cache visible to instruction fetches.
#[cfg(feature = "linux")]
unsafe fn sync_icache(range: core::ops::Range<u64>) {
	let ctr: u64;
	unsafe {
		asm!("mrs {}, ctr_el0", out(reg) ctr, options(nomem, nostack));
	}
	let line = 4u64 << ((ctr >> 16) & 0xf);

	let mut addr = range.start & !(line - 1);
	while addr < range.end {
		unsafe {
			asm!("dc cvau, {}", in(reg) addr, options(nostack));
		}
		addr += line;
	}

	unsafe {
		asm!("dsb ish", "ic iallu", "dsb ish", "isb", options(nostack));
	}
}

unsafe fn enter_kernel(stack: *mut u8, entry: *const (), raw_boot_info: &'static RawBootInfo) -> ! {
	// Check expected signature of entry function
	let entry: Entry = {
		let entry: unsafe extern "C" fn(raw_boot_info: &'static RawBootInfo, cpu_id: u32) -> ! =
			unsafe { core::mem::transmute(entry) };
		entry
	};

	info!("Entering kernel at {entry:p}, stack at {stack:p}, raw_boot_info at {raw_boot_info:p}");

	// Memory barrier
	CONSOLE.lock().get().wait_empty();
	dsb(SY);
	isb(SY);
	dmb(SY);
	dsb(NSH);

	unsafe {
		asm!(
			"mov sp, {stack}",
			"br {entry}",
			stack = in(reg) stack,
			entry = in(reg) entry,
			in("x0") raw_boot_info,
			in("x1") 0,
			options(noreturn)
		)
	}
}
