#![allow(unused)]

use alloc::vec::Vec;
use core::arch::asm;
use core::str;

use hermit_dtb::Dtb;
use hermit_sync::OnceCell;
use time::OffsetDateTime;

use crate::arch::aarch64::mm::paging::{self, BasePageSize, PageSize, PageTableEntryFlags};
use crate::arch::aarch64::mm::{virtualmem, PhysAddr, VirtAddr};
use crate::kernel::boot_info;

static PL031_ADDRESS: OnceCell<VirtAddr> = OnceCell::new();
static BOOT_TIME: OnceCell<u64> = OnceCell::new();

const RTC_DR: usize = 0x00;
const RTC_MR: usize = 0x04;
const RTC_LR: usize = 0x08;
const RTC_CR: usize = 0x0c;
/// Interrupt mask and set register
const RTC_IRQ_MASK: usize = 0x10;
/// Raw interrupt status
const RTC_RAW_IRQ_STATUS: usize = 0x14;
/// Masked interrupt status
const RTC_MASK_IRQ_STATUS: usize = 0x18;
/// Interrupt clear register
const RTC_IRQ_CLEAR: usize = 0x1c;

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

	value
}

pub fn init() {
	let dtb = unsafe {
		Dtb::from_raw(core::ptr::with_exposed_provenance(
			boot_info().hardware_info.device_tree.unwrap().get() as usize,
		))
		.expect(".dtb file has invalid header")
	};

	for node in dtb.enum_subnodes("/") {
		let parts: Vec<_> = node.split('@').collect();

		if let Some(compatible) = dtb.get_property(parts.first().unwrap(), "compatible") {
			if str::from_utf8(compatible).unwrap().contains("pl031") {
				let reg = dtb.get_property(parts.first().unwrap(), "reg").unwrap();
				let (slice, residual_slice) = reg.split_at(core::mem::size_of::<u64>());
				let addr = PhysAddr(u64::from_be_bytes(slice.try_into().unwrap()));
				let (slice, _residual_slice) = residual_slice.split_at(core::mem::size_of::<u64>());
				let size = u64::from_be_bytes(slice.try_into().unwrap());

				debug!("Found RTC at {:p} (size {:#X})", addr, size);

				let pl031_address = virtualmem::allocate_aligned(
					size.try_into().unwrap(),
					BasePageSize::SIZE.try_into().unwrap(),
				)
				.unwrap();
				PL031_ADDRESS.set(pl031_address).unwrap();
				debug!("Mapping RTC to virtual address {pl031_address:p}",);

				let mut flags = PageTableEntryFlags::empty();
				flags.device().writable().execute_disable();
				paging::map::<BasePageSize>(
					pl031_address,
					addr,
					(size / BasePageSize::SIZE).try_into().unwrap(),
					flags,
				);

				let boot_time =
					OffsetDateTime::from_unix_timestamp(rtc_read(RTC_DR) as i64).unwrap();
				info!("Hermit booted on {boot_time}");

				let micros = u64::try_from(boot_time.unix_timestamp_nanos() / 1000).unwrap();
				let current_ticks = super::processor::get_timer_ticks();
				BOOT_TIME.set(micros - current_ticks).unwrap();

				return;
			}
		}
	}

	PL031_ADDRESS.set(VirtAddr::zero()).unwrap();
	BOOT_TIME.set(0).unwrap();
}

/// Returns the current time in microseconds since UNIX epoch.
pub fn now_micros() -> u64 {
	*BOOT_TIME.get().unwrap() + super::processor::get_timer_ticks()
}
