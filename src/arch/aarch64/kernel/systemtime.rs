use core::arch::asm;

use free_list::PageLayout;
use hermit_sync::OnceCell;
use memory_addresses::{PhysAddr, VirtAddr};
use time::OffsetDateTime;

use crate::arch::aarch64::mm::paging::{self, BasePageSize, PageSize, PageTableEntryFlags};
use crate::env::{self, BootInfoExt};
use crate::mm::{PageAlloc, PageRangeAllocator};

static PL031_ADDRESS: OnceCell<VirtAddr> = OnceCell::new();
static BOOT_TIME: OnceCell<u64> = OnceCell::new();

#[expect(dead_code)]
mod reg {
	pub const RTC_DR: usize = 0x00;
	pub const RTC_MR: usize = 0x04;
	pub const RTC_LR: usize = 0x08;
	pub const RTC_CR: usize = 0x0c;
	/// Interrupt mask and set register
	pub const RTC_IRQ_MASK: usize = 0x10;
	/// Raw interrupt status
	pub const RTC_RAW_IRQ_STATUS: usize = 0x14;
	/// Masked interrupt status
	pub const RTC_MASK_IRQ_STATUS: usize = 0x18;
	/// Interrupt clear register
	pub const RTC_IRQ_CLEAR: usize = 0x1c;
}

#[inline]
fn rtc_read(off: usize) -> u32 {
	let value: u32;

	// we have to use inline assembly to guarantee 32bit memory access
	unsafe {
		asm!("ldar {value:w}, [{addr}]",
			value = out(reg) value,
			addr = in(reg) (PL031_ADDRESS.get().unwrap().as_usize() + off),
			options(nostack, readonly),
		);
	}

	u32::from_le(value)
}

fn boot_time() -> OffsetDateTime {
	#[cfg(feature = "uhyve")]
	if let Some(boot_time) = env::start_info().uhyve_boot_time() {
		return boot_time;
	}

	let fdt = env::start_info().fdt().unwrap();
	let Some(pl031_node) = fdt.find_compatible(&["arm,pl031"]) else {
		error!("Could not find PL031 Real Time Clock to determine the boot time.");
		return OffsetDateTime::UNIX_EPOCH;
	};

	let reg = pl031_node.reg().unwrap().next().unwrap();
	let addr = PhysAddr::from(reg.starting_address.addr());
	let size = u64::try_from(reg.size.unwrap()).unwrap();

	debug!("Found RTC at {addr:p} (size {size:#X})");

	let layout = PageLayout::from_size(size.try_into().unwrap()).unwrap();
	let page_range = PageAlloc::allocate(layout).unwrap();
	let pl031_address = VirtAddr::from(page_range.start());
	PL031_ADDRESS.set(pl031_address).unwrap();
	debug!("Mapping RTC to virtual address {pl031_address:p}");

	let mut flags = PageTableEntryFlags::empty();
	flags.device().writable().execute_disable();
	paging::map::<BasePageSize>(
		pl031_address,
		addr,
		(size / BasePageSize::SIZE).try_into().unwrap(),
		flags,
	);

	OffsetDateTime::from_unix_timestamp(rtc_read(reg::RTC_DR).into()).unwrap()
}

pub fn init() {
	let boot_time = boot_time();
	info!("Hermit booted on {boot_time}");

	BOOT_TIME
		.set(u64::try_from(boot_time.unix_timestamp_nanos() / 1000).unwrap())
		.unwrap();
}

/// Returns the current time in microseconds since UNIX epoch.
pub fn now_micros() -> u64 {
	*BOOT_TIME.get().unwrap() + super::processor::get_timer_ticks()
}
