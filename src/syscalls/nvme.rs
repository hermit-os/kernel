use vroom::{Dma, IoQueuePairId, Namespace, NamespaceId};

use crate::drivers::pci::get_nvme_driver;

// TODO: error messages
#[derive(Debug)]
pub(crate) enum SysNvmeError {
	ZeroPointerParameter = 1,
	DeviceDoesNotExist = 2,
	NamespaceDoesNotExist = 3,
	MaxNumberOfQueuesReached = 4,
	CouldNotCreateIoQueuePair = 5,
	CouldNotDeleteIoQueuePair = 6,
	CouldNotFindIoQueuePair = 7,
	BufferIncorrectlySized = 8,
	CouldNotAllocateBuffer = 9,
	CouldNotDeallocateBuffer = 10,
	CouldNotReadFromIoQueuePair = 11,
	CouldNotWriteToIoQueuePair = 12,
	CouldNotClearNamespace = 13,
}

#[hermit_macro::system]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_nvme_number_of_namespaces(result: *mut u32) -> usize {
	fn inner(result: *mut u32) -> Result<(), SysNvmeError> {
		if result.is_null() {
			return Err(SysNvmeError::ZeroPointerParameter);
		}
		let result = unsafe { &mut *result };
		let driver = get_nvme_driver().ok_or(SysNvmeError::DeviceDoesNotExist)?;
		let number_of_namespaces = driver.lock().namespace_ids().len() as u32;
		*result = number_of_namespaces;
		Ok(())
	}
	match inner(result) {
		Ok(()) => 0,
		Err(error) => error as usize,
	}
}

#[hermit_macro::system]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_nvme_namespace_ids(
	vec_pointer: *mut NamespaceId,
	length: u32,
) -> usize {
	fn inner(vec_pointer: *mut NamespaceId, length: u32) -> Result<(), SysNvmeError> {
		if vec_pointer.is_null() {
			return Err(SysNvmeError::ZeroPointerParameter);
		}
		let driver = get_nvme_driver().ok_or(SysNvmeError::DeviceDoesNotExist)?;
		let namespace_ids = driver.lock().namespace_ids();
		if namespace_ids.len() != length as usize {
			return Err(SysNvmeError::BufferIncorrectlySized);
		}
		for (i, namespace_id) in namespace_ids.iter().enumerate().take(length as usize) {
			let pointer = unsafe { vec_pointer.add(i) };
			unsafe { *pointer = *namespace_id };
		}
		Ok(())
	}
	match inner(vec_pointer, length) {
		Ok(()) => 0,
		Err(error) => error as usize,
	}
}

#[hermit_macro::system]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_nvme_namespace(
	namespace_id: &NamespaceId,
	result: *mut Namespace,
) -> usize {
	fn inner(namespace_id: &NamespaceId, result: *mut Namespace) -> Result<(), SysNvmeError> {
		if result.is_null() {
			return Err(SysNvmeError::ZeroPointerParameter);
		}
		let result = unsafe { &mut *result };
		let driver = get_nvme_driver().ok_or(SysNvmeError::DeviceDoesNotExist)?;
		let lock = driver.lock();
		let namespace = lock.namespace(namespace_id)?;
		*result = namespace;
		Ok(())
	}
	match inner(namespace_id, result) {
		Ok(()) => 0,
		Err(error) => error as usize,
	}
}

#[hermit_macro::system]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_nvme_clear_namespace(namespace_id: &NamespaceId) -> usize {
	fn inner(namespace_id: &NamespaceId) -> Result<(), SysNvmeError> {
		let driver = get_nvme_driver().ok_or(SysNvmeError::DeviceDoesNotExist)?;
		let lock = driver.lock();
		lock.clear_namespace(namespace_id)
	}
	match inner(namespace_id) {
		Ok(()) => 0,
		Err(error) => error as usize,
	}
}

#[hermit_macro::system]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_nvme_maximum_transfer_size(result: *mut usize) -> usize {
	fn inner(result: *mut usize) -> Result<(), SysNvmeError> {
		if result.is_null() {
			return Err(SysNvmeError::ZeroPointerParameter);
		}
		let result = unsafe { &mut *result };
		let driver = get_nvme_driver().ok_or(SysNvmeError::DeviceDoesNotExist)?;
		let maximum_transfer_size = driver.lock().maximum_transfer_size();
		*result = maximum_transfer_size;
		Ok(())
	}
	match inner(result) {
		Ok(()) => 0,
		Err(error) => error as usize,
	}
}

#[hermit_macro::system]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_nvme_maximum_number_of_io_queue_pairs(result: *mut u16) -> usize {
	fn inner(result: *mut u16) -> Result<(), SysNvmeError> {
		if result.is_null() {
			return Err(SysNvmeError::ZeroPointerParameter);
		}
		let result = unsafe { &mut *result };
		let driver = get_nvme_driver().ok_or(SysNvmeError::DeviceDoesNotExist)?;
		let maximum_number_of_io_queue_pairs = driver.lock().maximum_number_of_io_queue_pairs();
		*result = maximum_number_of_io_queue_pairs;
		Ok(())
	}
	match inner(result) {
		Ok(()) => 0,
		Err(error) => error as usize,
	}
}

#[hermit_macro::system]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_nvme_maximum_queue_entries_supported(result: *mut u32) -> usize {
	fn inner(result: *mut u32) -> Result<(), SysNvmeError> {
		if result.is_null() {
			return Err(SysNvmeError::ZeroPointerParameter);
		}
		let result = unsafe { &mut *result };
		let driver = get_nvme_driver().ok_or(SysNvmeError::DeviceDoesNotExist)?;
		let maximum_queue_entries_supported = driver.lock().maximum_queue_entries_supported();
		*result = maximum_queue_entries_supported;
		Ok(())
	}
	match inner(result) {
		Ok(()) => 0,
		Err(error) => error as usize,
	}
}

#[hermit_macro::system]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_nvme_create_io_queue_pair(
	namespace_id: &NamespaceId,
	number_of_entries: u32,
	resulting_io_queue_pair_id: *mut IoQueuePairId,
) -> usize {
	fn inner(
		namespace_id: &NamespaceId,
		number_of_entries: u32,
		resulting_io_queue_pair_id: *mut IoQueuePairId,
	) -> Result<(), SysNvmeError> {
		if resulting_io_queue_pair_id.is_null() {
			return Err(SysNvmeError::ZeroPointerParameter);
		}
		let resulting_io_queue_pair_id = unsafe { &mut *resulting_io_queue_pair_id };
		let driver = get_nvme_driver().ok_or(SysNvmeError::DeviceDoesNotExist)?;
		let io_queue_pair_id = driver
			.lock()
			.create_io_queue_pair(namespace_id, number_of_entries)?;
		*resulting_io_queue_pair_id = io_queue_pair_id;
		Ok(())
	}
	match inner(namespace_id, number_of_entries, resulting_io_queue_pair_id) {
		Ok(()) => 0,
		Err(error) => error as usize,
	}
}

#[hermit_macro::system]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_nvme_delete_io_queue_pair(io_queue_pair_id: IoQueuePairId) -> usize {
	fn inner(io_queue_pair_id: IoQueuePairId) -> Result<(), SysNvmeError> {
		let driver = get_nvme_driver().ok_or(SysNvmeError::DeviceDoesNotExist)?;
		driver.lock().delete_io_queue_pair(io_queue_pair_id)
	}
	match inner(io_queue_pair_id) {
		Ok(()) => 0,
		Err(error) => error as usize,
	}
}

#[hermit_macro::system]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_nvme_allocate_buffer(
	io_queue_pair_id: &IoQueuePairId,
	size: usize,
	resulting_buffer: *mut Dma<u8>,
) -> usize {
	fn inner(
		io_queue_pair_id: &IoQueuePairId,
		number_of_elements: usize,
		resulting_buffer_pointer: *mut Dma<u8>,
	) -> Result<(), SysNvmeError> {
		let resulting_buffer_pointer = unsafe { &mut *resulting_buffer_pointer };
		let driver = get_nvme_driver().ok_or(SysNvmeError::DeviceDoesNotExist)?;
		let buffer = driver
			.lock()
			.allocate_buffer(io_queue_pair_id, number_of_elements)?;
		*resulting_buffer_pointer = buffer;
		Ok(())
	}
	match inner(io_queue_pair_id, size, resulting_buffer) {
		Ok(()) => 0,
		Err(error) => error as usize,
	}
}

#[hermit_macro::system]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_nvme_deallocate_buffer(
	io_queue_pair_id: &IoQueuePairId,
	buffer: *mut Dma<u8>,
) -> usize {
	fn inner(io_queue_pair_id: &IoQueuePairId, buffer: *mut Dma<u8>) -> Result<(), SysNvmeError> {
		let _ = buffer;
		let buffer: Dma<u8> = unsafe { core::ptr::read(buffer) };
		let driver = get_nvme_driver().ok_or(SysNvmeError::DeviceDoesNotExist)?;
		driver.lock().deallocate_buffer(io_queue_pair_id, buffer)
	}
	match inner(io_queue_pair_id, buffer) {
		Ok(()) => 0,
		Err(error) => error as usize,
	}
}

#[hermit_macro::system]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_nvme_read_from_io_queue_pair(
	io_queue_pair_id: &IoQueuePairId,
	buffer: *mut Dma<u8>,
	logical_block_address: u64,
) -> usize {
	fn inner(
		io_queue_pair_id: &IoQueuePairId,
		buffer: *mut Dma<u8>,
		logical_block_address: u64,
	) -> Result<(), SysNvmeError> {
		let buffer = unsafe { &mut *buffer };
		let driver = get_nvme_driver().ok_or(SysNvmeError::DeviceDoesNotExist)?;
		driver
			.lock()
			.read_from_io_queue_pair(io_queue_pair_id, buffer, logical_block_address)
	}
	match inner(io_queue_pair_id, buffer, logical_block_address) {
		Ok(()) => 0,
		Err(error) => error as usize,
	}
}

#[hermit_macro::system]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_nvme_write_to_io_queue_pair(
	io_queue_pair_id: &IoQueuePairId,
	buffer: *const Dma<u8>,
	logical_block_address: u64,
) -> usize {
	fn inner(
		io_queue_pair_id: &IoQueuePairId,
		buffer: *const Dma<u8>,
		logical_block_address: u64,
	) -> Result<(), SysNvmeError> {
		let buffer = unsafe { &*buffer };
		let driver = get_nvme_driver().ok_or(SysNvmeError::DeviceDoesNotExist)?;
		driver
			.lock()
			.write_to_io_queue_pair(io_queue_pair_id, buffer, logical_block_address)
	}
	match inner(io_queue_pair_id, buffer, logical_block_address) {
		Ok(()) => 0,
		Err(error) => error as usize,
	}
}

#[hermit_macro::system]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_nvme_submit_read_to_io_queue_pair(
	io_queue_pair_id: &IoQueuePairId,
	buffer: *mut Dma<u8>,
	logical_block_address: u64,
) -> usize {
	fn inner(
		io_queue_pair_id: &IoQueuePairId,
		buffer: *mut Dma<u8>,
		logical_block_address: u64,
	) -> Result<(), SysNvmeError> {
		let buffer = unsafe { &mut *buffer };
		let driver = get_nvme_driver().ok_or(SysNvmeError::DeviceDoesNotExist)?;
		driver
			.lock()
			.submit_read_to_io_queue_pair(io_queue_pair_id, buffer, logical_block_address)
	}
	match inner(io_queue_pair_id, buffer, logical_block_address) {
		Ok(()) => 0,
		Err(error) => error as usize,
	}
}

#[hermit_macro::system]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_nvme_submit_write_to_io_queue_pair(
	io_queue_pair_id: &IoQueuePairId,
	buffer: *const Dma<u8>,
	logical_block_address: u64,
) -> usize {
	fn inner(
		io_queue_pair_id: &IoQueuePairId,
		buffer: *const Dma<u8>,
		logical_block_address: u64,
	) -> Result<(), SysNvmeError> {
		let buffer = unsafe { &*buffer };
		let driver = get_nvme_driver().ok_or(SysNvmeError::DeviceDoesNotExist)?;
		driver
			.lock()
			.submit_write_to_io_queue_pair(io_queue_pair_id, buffer, logical_block_address)
	}
	match inner(io_queue_pair_id, buffer, logical_block_address) {
		Ok(()) => 0,
		Err(error) => error as usize,
	}
}

#[hermit_macro::system]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_nvme_complete_io_with_io_queue_pair(
	io_queue_pair_id: &IoQueuePairId,
) -> usize {
	fn inner(io_queue_pair_id: &IoQueuePairId) -> Result<(), SysNvmeError> {
		let driver = get_nvme_driver().ok_or(SysNvmeError::DeviceDoesNotExist)?;
		driver
			.lock()
			.complete_io_with_io_queue_pair(io_queue_pair_id)
	}
	match inner(io_queue_pair_id) {
		Ok(()) => 0,
		Err(error) => error as usize,
	}
}
