use alloc::sync::Arc;
use alloc::vec::Vec;
use core::hint;
use core::ptr::{self, NonNull};

use acpi::aml::AmlError;
use acpi::{Handle, Handler, PciAddress, PhysicalMapping};
use align_address::Align;
use hermit_sync::{RawSpinMutex, SpinMutex};
use lock_api::RawMutex;
#[cfg(target_arch = "x86_64")]
use x86_64::instructions::port::Port;

use crate::arch::kernel::{core_local, processor};
use crate::arch::mm::paging::{self, BasePageSize};
use crate::scheduler::PerCoreSchedulerExt;

#[derive(Default, Clone, Debug)]
pub struct AcpiHandler {
	state: Arc<State>,
}

#[derive(Default, Debug)]
struct State {
	mutexes: SpinMutex<Vec<Arc<RawSpinMutex>>>,
}

impl Handler for AcpiHandler {
	unsafe fn map_physical_region<T>(
		&self,
		physical_address: usize,
		size: usize,
	) -> PhysicalMapping<Self, T> {
		let physical_start = physical_address.align_down(0x1000);
		let physical_end = (physical_address + size).align_up(0x1000);
		let mapped_length = physical_end - physical_start;
		let handler = self.clone();

		trace!(
			"Mapping physical region...   paddr = {physical_start:#x}, len = {mapped_length:#x}"
		);

		for paddr in (physical_start..physical_start + mapped_length).step_by(0x1000) {
			paging::identity_map::<BasePageSize>(paddr.into());
		}

		let virtual_start = ptr::with_exposed_provenance_mut(physical_address);
		let virtual_start = NonNull::new(virtual_start).unwrap();
		let region_length = size;

		PhysicalMapping {
			physical_start,
			virtual_start,
			region_length,
			mapped_length,
			handler,
		}
	}

	fn unmap_physical_region<T>(region: &PhysicalMapping<Self, T>) {
		trace!(
			"Unmapping physical region... paddr = {:#x}, len = {:#x}",
			region.physical_start, region.mapped_length
		);
		// We don't unmap currently.
	}

	fn read_u8(&self, address: usize) -> u8 {
		trace!("read_u8({address:#x})");
		let ptr = ptr::with_exposed_provenance(address);
		unsafe { *ptr }
	}

	fn read_u16(&self, address: usize) -> u16 {
		trace!("read_u16({address:#x})");
		let ptr = ptr::with_exposed_provenance(address);
		unsafe { *ptr }
	}

	fn read_u32(&self, address: usize) -> u32 {
		trace!("read_u32({address:#x})");
		let ptr = ptr::with_exposed_provenance(address);
		unsafe { *ptr }
	}

	fn read_u64(&self, address: usize) -> u64 {
		trace!("read_u64({address:#x})");
		let ptr = ptr::with_exposed_provenance(address);
		unsafe { *ptr }
	}

	fn write_u8(&self, address: usize, value: u8) {
		trace!("write_u8({address:#x}, {value:#x})");
		let ptr = ptr::with_exposed_provenance_mut(address);
		unsafe {
			*ptr = value;
		}
	}

	fn write_u16(&self, address: usize, value: u16) {
		trace!("write_u16({address:#x}, {value:#x})");
		let ptr = ptr::with_exposed_provenance_mut(address);
		unsafe {
			*ptr = value;
		}
	}

	fn write_u32(&self, address: usize, value: u32) {
		trace!("write_u32({address:#x}, {value:#x})");
		let ptr = ptr::with_exposed_provenance_mut(address);
		unsafe {
			*ptr = value;
		}
	}

	fn write_u64(&self, address: usize, value: u64) {
		trace!("write_u64({address:#x}, {value:#x})");
		let ptr = ptr::with_exposed_provenance_mut(address);
		unsafe {
			*ptr = value;
		}
	}

	fn read_io_u8(&self, port: u16) -> u8 {
		trace!("read_io_u8({port:#x})");
		cfg_select! {
			target_arch = "x86_64" => unsafe { Port::new(port).read() },
			_ => unimplemented!(),
		}
	}

	fn read_io_u16(&self, port: u16) -> u16 {
		trace!("read_io_u16({port:#x})");
		cfg_select! {
			target_arch = "x86_64" => unsafe { Port::new(port).read() },
			_ => unimplemented!(),
		}
	}

	fn read_io_u32(&self, port: u16) -> u32 {
		trace!("read_io_u32({port:#x})");
		cfg_select! {
			target_arch = "x86_64" => unsafe { Port::new(port).read() },
			_ => unimplemented!(),
		}
	}

	fn write_io_u8(&self, port: u16, value: u8) {
		trace!("write_io_u8({port:#x}, {value:#x})");
		cfg_select! {
			target_arch = "x86_64" => unsafe { Port::new(port).write(value) },
			_ => unimplemented!(),
		}
	}

	fn write_io_u16(&self, port: u16, value: u16) {
		trace!("write_io_u16({port:#x}, {value:#x})");
		cfg_select! {
			target_arch = "x86_64" => unsafe { Port::new(port).write(value) },
			_ => unimplemented!(),
		}
	}

	fn write_io_u32(&self, port: u16, value: u32) {
		trace!("write_io_u32({port:#x}, {value:#x})");
		cfg_select! {
			target_arch = "x86_64" => unsafe { Port::new(port).write(value) },
			_ => unimplemented!(),
		}
	}

	fn read_pci_u8(&self, address: PciAddress, offset: u16) -> u8 {
		trace!("read_pci_u8({address}, {offset:#x})");
		todo!()
	}

	fn read_pci_u16(&self, address: PciAddress, offset: u16) -> u16 {
		trace!("read_pci_u16({address}, {offset:#x})");
		todo!("needs an arch-unified PCI interface")
	}

	fn read_pci_u32(&self, address: PciAddress, offset: u16) -> u32 {
		trace!("read_pci_u32({address}, {offset:#x})");
		todo!("needs an arch-unified PCI interface")
	}

	fn write_pci_u8(&self, address: PciAddress, offset: u16, value: u8) {
		trace!("write_pci_u8({address}, {offset:#x}, {value:#x})");
		todo!("needs an arch-unified PCI interface")
	}

	fn write_pci_u16(&self, address: PciAddress, offset: u16, value: u16) {
		trace!("write_pci_u16({address}, {offset:#x}, {value:#x})");
		todo!("needs an arch-unified PCI interface")
	}

	fn write_pci_u32(&self, address: PciAddress, offset: u16, value: u32) {
		trace!("write_pci_u32({address}, {offset:#x}, {value:#x})");
		todo!("needs an arch-unified PCI interface")
	}

	fn nanos_since_boot(&self) -> u64 {
		trace!("nanos_since_boot()");
		processor::get_timer_ticks() * 1000
	}

	fn stall(&self, microseconds: u64) {
		trace!("stall({microseconds}µs)");

		// FIXME: This is taken from x86-64's `udelay()`.
		// We should make `udelay()` cross-architecture, instead.
		let end = processor::get_timestamp() + u64::from(processor::get_frequency()) * microseconds;
		while processor::get_timestamp() < end {
			hint::spin_loop();
		}
	}

	fn sleep(&self, milliseconds: u64) {
		trace!("sleep({milliseconds}ms)");

		// FIXME: This is taken from `usleep()`.
		// We should create an always-sleeping function and use that here.
		let core_scheduler = core_local::core_scheduler();
		let wakeup_time = processor::get_timer_ticks() + milliseconds * 1000;
		core_scheduler.block_current_task(Some(wakeup_time));
		core_scheduler.reschedule();
	}

	fn create_mutex(&self) -> Handle {
		trace!("create_mutex()");
		let mut mutexes = self.state.mutexes.lock();

		let i = u32::try_from(mutexes.len()).unwrap();
		mutexes.push(Arc::new(RawSpinMutex::INIT));

		Handle(i)
	}

	fn acquire(&self, mutex: Handle, timeout: u16) -> Result<(), AmlError> {
		// FIXME: This mutex should be reentrant and suspend threads. To do that, we should rework
		// `crate::synch::recmutex` with `lock_api::ReentrantMutex` in a way that handles timeouts.
		// The implementation should be based on futexes and might be used to provide pthread APIs
		// in the future.

		trace!("acquire({mutex:?}, {timeout}ms)");

		let raw_mutex = self.raw_mutex(mutex)?;

		match timeout {
			0 => match raw_mutex.try_lock() {
				true => Ok(()),
				false => Err(AmlError::MutexAcquireTimeout),
			},
			1..0xffff => {
				let end = processor::get_timestamp()
					+ u64::from(processor::get_frequency()) * u64::from(timeout);
				while processor::get_timestamp() < end {
					if raw_mutex.try_lock() {
						return Ok(());
					}

					self.sleep(1);
				}

				Err(AmlError::MutexAcquireTimeout)
			}
			0xffff => {
				raw_mutex.lock();
				Ok(())
			}
		}
	}

	fn release(&self, mutex: Handle) {
		trace!("release({mutex:?})");

		let raw_mutex = self.raw_mutex(mutex).unwrap();
		unsafe { raw_mutex.unlock() }
	}
}

impl AcpiHandler {
	fn raw_mutex(&self, mutex: Handle) -> Result<Arc<RawSpinMutex>, AmlError> {
		let mutexes = self.state.mutexes.lock();
		let index = usize::try_from(mutex.0).map_err(|_| AmlError::IndexOutOfBounds)?;
		let mutex = mutexes.get(index).ok_or(AmlError::IndexOutOfBounds)?;
		Ok(mutex.clone())
	}
}
