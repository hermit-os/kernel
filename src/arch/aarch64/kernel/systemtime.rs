#![allow(unused)]

use alloc::vec::Vec;
use core::arch::asm;
use core::str;

use arm_pl031::Rtc;
use hermit_dtb::Dtb;
use hermit_sync::OnceCell;
use memory_addresses::arch::aarch64::{PhysAddr, VirtAddr};
use time::OffsetDateTime;

use crate::arch::aarch64::mm::paging::{self, BasePageSize, PageSize, PageTableEntryFlags};
use crate::env;
use crate::mm::virtualmem;

static RTC_PL031: OnceCell<Rtc> = OnceCell::new();
static BOOT_TIME: OnceCell<u64> = OnceCell::new();

pub fn init() {
	let dtb = unsafe {
		Dtb::from_raw(core::ptr::with_exposed_provenance(
			env::boot_info().hardware_info.device_tree.unwrap().get() as usize,
		))
		.expect(".dtb file has invalid header")
	};

	for node in dtb.enum_subnodes("/") {
		let parts: Vec<_> = node.split('@').collect();

		if let Some(compatible) = dtb.get_property(parts.first().unwrap(), "compatible") {
			if str::from_utf8(compatible).unwrap().contains("pl031") {
				let reg = dtb.get_property(parts.first().unwrap(), "reg").unwrap();
				let (slice, residual_slice) = reg.split_at(core::mem::size_of::<u64>());
				let addr = PhysAddr::new(u64::from_be_bytes(slice.try_into().unwrap()));
				let (slice, _residual_slice) = residual_slice.split_at(core::mem::size_of::<u64>());
				let size = u64::from_be_bytes(slice.try_into().unwrap());

				debug!("Found RTC at {addr:p} (size {size:#X})");

				let pl031_address = virtualmem::allocate_aligned(
					size.try_into().unwrap(),
					BasePageSize::SIZE.try_into().unwrap(),
				)
				.unwrap();

				let mut flags = PageTableEntryFlags::empty();
				flags.device().writable().execute_disable();
				paging::map::<BasePageSize>(
					pl031_address,
					addr,
					(size / BasePageSize::SIZE).try_into().unwrap(),
					flags,
				);

				debug!("Mapping RTC to virtual address {pl031_address:p}");

				let rtc = unsafe { Rtc::new(pl031_address.as_mut_ptr()) };
				let boot_time =
					OffsetDateTime::from_unix_timestamp(rtc.get_unix_timestamp().into()).unwrap();
				info!("Hermit booted on {boot_time}");

				let micros = u64::try_from(boot_time.unix_timestamp_nanos() / 1000).unwrap();
				let current_ticks = super::processor::get_timer_ticks();

				assert!(
					BOOT_TIME.set(micros - current_ticks).is_err(),
					"Unable to set BOOT_TIME"
				);
				assert!(RTC_PL031.set(rtc).is_err(), "Unable to set RTC_PL031");

				return;
			}
		}
	}

	BOOT_TIME.set(0).unwrap();
}

/// Returns the current time in microseconds since UNIX epoch.
pub fn now_micros() -> u64 {
	*BOOT_TIME.get().unwrap() + super::processor::get_timer_ticks()
}
