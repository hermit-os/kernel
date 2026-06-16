use memory_addresses::VirtAddr;

use crate::arch::mm::paging;

#[hermit_macro::system]
#[unsafe(no_mangle)]
pub extern "C" fn sys_virt_addr_to_phys_addr(virt_addr: usize) -> usize {
	let virt_addr = VirtAddr::from(virt_addr);
	let phys_addr = paging::virtual_to_physical(virt_addr);
	phys_addr.map_or(Default::default(), |phys_addr| phys_addr.as_usize())
}
