use alloc::boxed::Box;
use alloc::vec::Vec;
use core::alloc::{Allocator, Layout};
use core::ptr::NonNull;

use ahash::RandomState;
use hashbrown::HashMap;
use hermit_sync::{InterruptTicketMutex, Lazy};
use memory_addresses::VirtAddr;
use pci_types::InterruptLine;
use vroom::{Dma, IoQueuePair, IoQueuePairId, Namespace, NamespaceId, NvmeDevice};

use crate::arch::mm::paging::{virtual_to_physical, BasePageSize, PageSize};
use crate::arch::pci::PciConfigRegion;
use crate::drivers::pci::PciDevice;
use crate::drivers::Driver;
use crate::mm::device_alloc::DeviceAlloc;
use crate::syscalls::nvme::SysNvmeError;

pub(crate) struct NvmeDriver {
	irq: InterruptLine,
	device: InterruptTicketMutex<NvmeDevice<NvmeAllocator>>,
	// TODO: Replace with a concurrent hashmap. See crate::synch::futex.
	io_queue_pairs:
		Lazy<InterruptTicketMutex<HashMap<IoQueuePairId, IoQueuePair<NvmeAllocator>, RandomState>>>,
}

impl NvmeDriver {
	pub(crate) fn init(pci_device: &PciDevice<PciConfigRegion>) -> Result<Self, ()> {
		let allocator: NvmeAllocator = NvmeAllocator {
			device_allocator: DeviceAlloc {},
			allocations: Lazy::new(|| {
				InterruptTicketMutex::new(HashMap::with_hasher(RandomState::with_seeds(0, 0, 0, 0)))
			}),
		};
		let (virtual_address, size) = pci_device.memory_map_bar(0, true).ok_or(())?;
		let nvme_device: NvmeDevice<NvmeAllocator> = NvmeDevice::new(
			virtual_address.as_mut_ptr(),
			size,
			BasePageSize::SIZE as usize,
			allocator,
		)
		.map_err(|_| ())?;
		let driver = Self {
			irq: pci_device
				.get_irq()
				.expect("NVMe driver: Could not get irq from device."),
			device: InterruptTicketMutex::new(nvme_device),
			io_queue_pairs: Lazy::new(|| {
				InterruptTicketMutex::new(HashMap::with_hasher(RandomState::with_seeds(0, 0, 0, 0)))
			}),
		};
		Ok(driver)
	}

	pub(crate) fn namespace_ids(&self) -> Vec<NamespaceId> {
		self.device.lock().namespace_ids()
	}

	pub(crate) fn namespace(&self, namespace_id: &NamespaceId) -> Result<Namespace, SysNvmeError> {
		self.device
			.lock()
			.namespace(namespace_id)
			.map_err(|_| SysNvmeError::NamespaceDoesNotExist)
			.copied()
	}

	pub(crate) fn clear_namespace(&self, namespace_id: &NamespaceId) -> Result<(), SysNvmeError> {
		self.device
			.lock()
			.clear_namespace(namespace_id)
			.map_err(|_| SysNvmeError::CouldNotClearNamespace)
	}

	pub(crate) fn maximum_transfer_size(&self) -> usize {
		self.device
			.lock()
			.controller_information()
			.maximum_transfer_size
	}

	pub(crate) fn maximum_number_of_io_queue_pairs(&self) -> u16 {
		self.device
			.lock()
			.controller_information()
			.maximum_number_of_io_queue_pairs
	}

	pub(crate) fn maximum_queue_entries_supported(&self) -> u32 {
		self.device
			.lock()
			.controller_information()
			.maximum_queue_entries_supported
	}

	/// Creates an IO queue pair with a given number of entries for a namespace.
	pub(crate) fn create_io_queue_pair(
		&mut self,
		namespace_id: &NamespaceId,
		number_of_entries: u32,
	) -> Result<IoQueuePairId, SysNvmeError> {
		let mut device = self.device.lock();
		if !device.namespace_ids().contains(namespace_id) {
			return Err(SysNvmeError::NamespaceDoesNotExist);
		}
		let mut io_queue_pairs = self.io_queue_pairs.lock();
		if io_queue_pairs.len()
			>= device
				.controller_information()
				.maximum_number_of_io_queue_pairs
				.into()
		{
			return Err(SysNvmeError::MaxNumberOfQueuesReached);
		}
		let io_queue_pair = device
			.create_io_queue_pair(namespace_id, number_of_entries)
			.map_err(|_| SysNvmeError::CouldNotCreateIoQueuePair)?;
		let id = io_queue_pair.id();
		io_queue_pairs.insert(id, io_queue_pair);
		Ok(id)
	}

	/// Deletes an IO queue pair and frees its resources.
	pub(crate) fn delete_io_queue_pair(
		&mut self,
		io_queue_pair_id: IoQueuePairId,
	) -> Result<(), SysNvmeError> {
		let mut device = self.device.lock();
		let io_queue_pair = self
			.io_queue_pairs
			.lock()
			.remove(&io_queue_pair_id)
			.ok_or(SysNvmeError::CouldNotFindIoQueuePair)?;
		device
			.delete_io_queue_pair(io_queue_pair)
			.map_err(|_error| SysNvmeError::CouldNotDeleteIoQueuePair)
	}

	pub(crate) fn allocate_buffer<T>(
		&self,
		io_queue_pair_id: &IoQueuePairId,
		number_of_elements: usize,
	) -> Result<Dma<T>, SysNvmeError> {
		let mut io_queue_pairs = self.io_queue_pairs.lock();
		let io_queue_pair = io_queue_pairs
			.get_mut(io_queue_pair_id)
			.ok_or(SysNvmeError::CouldNotFindIoQueuePair)?;
		io_queue_pair
			.allocate_buffer(number_of_elements)
			.map_err(|_error| SysNvmeError::CouldNotAllocateBuffer)
	}

	pub(crate) fn deallocate_buffer<T>(
		&self,
		io_queue_pair_id: &IoQueuePairId,
		buffer: Dma<T>,
	) -> Result<(), SysNvmeError> {
		let mut io_queue_pairs = self.io_queue_pairs.lock();
		let io_queue_pair = io_queue_pairs
			.get_mut(io_queue_pair_id)
			.ok_or(SysNvmeError::CouldNotFindIoQueuePair)?;
		io_queue_pair
			.deallocate_buffer(buffer)
			.map_err(|_error| SysNvmeError::CouldNotDeallocateBuffer)
	}

	/// Reads from the IO queue pair with ID `io_queue_pair_id`
	/// into the `buffer` starting from the `logical_block_address`.
	pub(crate) fn read_from_io_queue_pair<T>(
		&mut self,
		io_queue_pair_id: &IoQueuePairId,
		buffer: &mut Dma<T>,
		logical_block_address: u64,
	) -> Result<(), SysNvmeError> {
		let mut io_queue_pairs = self.io_queue_pairs.lock();
		let io_queue_pair = io_queue_pairs
			.get_mut(io_queue_pair_id)
			.ok_or(SysNvmeError::CouldNotFindIoQueuePair)?;
		io_queue_pair
			.read(buffer, logical_block_address)
			.map_err(|_error| SysNvmeError::CouldNotReadFromIoQueuePair)?;
		Ok(())
	}

	/// Writes the `buffer` to the IO queue pair with ID `io_queue_pair_id`
	/// starting from the `logical_block_address`.
	pub(crate) fn write_to_io_queue_pair<T>(
		&mut self,
		io_queue_pair_id: &IoQueuePairId,
		buffer: &Dma<T>,
		logical_block_address: u64,
	) -> Result<(), SysNvmeError> {
		let mut io_queue_pairs = self.io_queue_pairs.lock();
		let io_queue_pair = io_queue_pairs
			.get_mut(io_queue_pair_id)
			.ok_or(SysNvmeError::CouldNotFindIoQueuePair)?;
		io_queue_pair
			.write(buffer, logical_block_address)
			.map_err(|_error| SysNvmeError::CouldNotWriteToIoQueuePair)?;
		Ok(())
	}

	/// Submits a read command to the IO queue pair with ID `io_queue_pair_id`
	/// that reads into the `buffer` starting from the `logical_block_address`.
	pub(crate) fn submit_read_to_io_queue_pair<T>(
		&mut self,
		io_queue_pair_id: &IoQueuePairId,
		buffer: &mut Dma<T>,
		logical_block_address: u64,
	) -> Result<(), SysNvmeError> {
		let mut io_queue_pairs = self.io_queue_pairs.lock();
		let io_queue_pair = io_queue_pairs
			.get_mut(io_queue_pair_id)
			.ok_or(SysNvmeError::CouldNotFindIoQueuePair)?;
		io_queue_pair
			.submit_read(buffer, logical_block_address)
			.map_err(|_error| SysNvmeError::CouldNotReadFromIoQueuePair)?;
		Ok(())
	}

	/// Submits a write command to the IO queue pair with ID `io_queue_pair_id`
	/// that writes the `buffer` starting from the `logical_block_address`.
	pub(crate) fn submit_write_to_io_queue_pair<T>(
		&mut self,
		io_queue_pair_id: &IoQueuePairId,
		buffer: &Dma<T>,
		logical_block_address: u64,
	) -> Result<(), SysNvmeError> {
		let mut io_queue_pairs = self.io_queue_pairs.lock();
		let io_queue_pair = io_queue_pairs
			.get_mut(io_queue_pair_id)
			.ok_or(SysNvmeError::CouldNotFindIoQueuePair)?;
		io_queue_pair
			.submit_write(buffer, logical_block_address)
			.map_err(|_error| SysNvmeError::CouldNotReadFromIoQueuePair)?;
		Ok(())
	}

	pub(crate) fn complete_io_with_io_queue_pair(
		&mut self,
		io_queue_pair_id: &IoQueuePairId,
	) -> Result<(), SysNvmeError> {
		let mut io_queue_pairs = self.io_queue_pairs.lock();
		let io_queue_pair = io_queue_pairs
			.get_mut(io_queue_pair_id)
			.ok_or(SysNvmeError::CouldNotFindIoQueuePair)?;
		io_queue_pair
			.complete_io()
			.map_err(|_error| SysNvmeError::CouldNotReadFromIoQueuePair)?;
		Ok(())
	}
}

pub(crate) struct NvmeAllocator {
	pub(crate) device_allocator: DeviceAlloc,
	// TODO: Replace with a concurrent hashmap. See crate::synch::futex.
	pub(crate) allocations: Lazy<InterruptTicketMutex<HashMap<usize, Layout, RandomState>>>,
}

impl vroom::Allocator for NvmeAllocator {
	fn allocate<T>(
		&self,
		layout: core::alloc::Layout,
	) -> Result<*mut [T], Box<dyn core::error::Error>> {
		debug!("NVMe driver: allocate size {:#x}", layout.size());
		let Ok(memory) = self.device_allocator.allocate(layout) else {
            return Err("NVMe driver: Could not allocate memory with device allocator.".into());
		};
		self.allocations
			.lock()
			.insert(memory.as_ptr().addr(), layout);
		let slice =
			unsafe { core::slice::from_raw_parts_mut(memory.as_mut_ptr().cast::<T>(), memory.len()) };
		Ok(core::ptr::from_mut::<[T]>(slice))
	}

	fn deallocate<T>(&self, slice: *mut [T]) -> Result<(), Box<dyn core::error::Error>> {
		let address = slice.as_mut_ptr() as usize;
		debug!("NVMe driver: deallocate address {address:#X}");
		let layout: Layout = match self.allocations.lock().remove(&address) {
			None => {
				return Err(
					"NVMe driver: The given address did not map to an address and a layout.
                    This mapping should have occurred during allocation."
						.into(),
				);
			}
			Some(layout) => layout,
		};
		let virtual_address = unsafe { NonNull::new_unchecked(address as *mut u8) };
		unsafe { self.device_allocator.deallocate(virtual_address, layout) };
		Ok(())
	}

	fn translate_virtual_to_physical<T>(
		&self,
		virtual_address: *const T,
	) -> Result<*const T, Box<dyn core::error::Error>> {
		let address = virtual_address as usize;
		debug!("NVMe driver: translate virtual address {address:#x}");
		let virtual_address: VirtAddr = VirtAddr::new(address as u64);
		let Some(physical_address) = virtual_to_physical(virtual_address) else {
            return Err("NVMe driver: The given virtual address could not be mapped to a physical one.".into());
        };
		Ok(physical_address.as_usize() as *mut T)
	}
}

impl Driver for NvmeDriver {
	fn get_interrupt_number(&self) -> InterruptLine {
		self.irq
	}

	fn get_name(&self) -> &'static str {
		"nvme"
	}
}
