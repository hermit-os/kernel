use arch::x86_64::kernel::apic;
use arch::x86_64::kernel::irq::*;
use arch::x86_64::kernel::pci::{self, PciAdapter, PciDriver};
use arch::x86_64::kernel::percore::core_scheduler;
use arch::x86_64::kernel::virtio_fs;

use arch::x86_64::mm::paging::{BasePageSize, PageSize};
use arch::x86_64::mm::{paging, virtualmem};

use alloc::boxed::Box;
use alloc::rc::Rc;
use alloc::vec::Vec;
use core::cell::RefCell;

use self::consts::*;

pub mod consts {
	/* Common configuration */
	pub const VIRTIO_PCI_CAP_COMMON_CFG: u32 = 1;
	/* Notifications */
	pub const VIRTIO_PCI_CAP_NOTIFY_CFG: u32 = 2;
	/* ISR Status */
	pub const VIRTIO_PCI_CAP_ISR_CFG: u32 = 3;
	/* Device specific configuration */
	pub const VIRTIO_PCI_CAP_DEVICE_CFG: u32 = 4;
	/* PCI configuration access */
	pub const VIRTIO_PCI_CAP_PCI_CFG: u32 = 5;

	pub const VIRTIO_F_RING_INDIRECT_DESC: u64 = 1 << 28;
	pub const VIRTIO_F_RING_EVENT_IDX: u64 = 1 << 29;
	pub const VIRTIO_F_VERSION_1: u64 = 1 << 32;
	pub const VIRTIO_F_ACCESS_PLATFORM: u64 = 1 << 33;
	pub const VIRTIO_F_RING_PACKED: u64 = 1 << 34;
	pub const VIRTIO_F_IN_ORDER: u64 = 1 << 35;
	pub const VIRTIO_F_ORDER_PLATFORM: u64 = 1 << 36;
	pub const VIRTIO_F_SR_IOV: u64 = 1 << 37;
	pub const VIRTIO_F_NOTIFICATION_DATA: u64 = 1 << 38;

	// Descriptor flags
	pub const VIRTQ_DESC_F_NEXT: u16 = 1; // Buffer continues via next field
	pub const VIRTQ_DESC_F_WRITE: u16 = 2; // Buffer is device write-only (instead of read-only)
	pub const VIRTQ_DESC_F_INDIRECT: u16 = 4; // Buffer contains list of virtq_desc
}

pub struct Virtq<'a> {
	index: u16,  // Index of vq in common config
	vqsize: u16, // Elements in ring/descrs
	// The actial descriptors (16 bytes each)
	virtq_desc: VirtqDescriptors,
	// A ring of available descriptor heads with free-running index
	avail: Rc<RefCell<VirtqAvail<'a>>>,
	// A ring of used descriptor heads with free-running index
	used: Rc<RefCell<VirtqUsed<'a>>>,
	// Address where queue index is written to on notify
	queue_notify_address: &'a mut u16,
}

impl<'a> Virtq<'a> {
	// TODO: are the lifetimes correct?
	fn new(
		index: u16,
		vqsize: u16,
		virtq_desc: Vec<Box<virtq_desc_raw>>,
		avail: VirtqAvail<'a>,
		used: VirtqUsed<'a>,
		queue_notify_address: &'a mut u16,
	) -> Self {
		Virtq {
			index,
			vqsize,
			virtq_desc: VirtqDescriptors::new(virtq_desc),
			avail: Rc::new(RefCell::new(avail)),
			used: Rc::new(RefCell::new(used)),
			queue_notify_address,
		}
	}

	pub fn new_from_common(
		index: u16,
		common_cfg: &mut virtio_pci_common_cfg,
		notify_cfg: &mut VirtioNotification,
	) -> Option<Self> {
		// 1.Write the virtqueue index to queue_select.
		common_cfg.queue_select = index;

		// 2.Read the virtqueue size from queue_size. This controls how big the virtqueue is (see 2.4 Virtqueues).
		//   If this field is 0, the virtqueue does not exist.
		let vqsize = common_cfg.queue_size as usize;
		if vqsize == 0 || vqsize > 32768 {
			return None;
		}
		debug!("Initializing virtqueue {}, of size {}", index, vqsize);

		// 3.Optionally, select a smaller virtqueue size and write it to queue_size.

		// 4.Allocate and zero Descriptor Table, Available and Used rings for the virtqueue in contiguous physical memory.
		// TODO: is this contiguous memory?
		// TODO: (from 2.6.13.1 Placing Buffers Into The Descriptor Table):
		//   In practice, d.next is usually used to chain free descriptors,
		//   and a separate count kept to check there are enough free descriptors before beginning the mappings.
		let desc_table = vec![
			virtq_desc_raw {
				addr: 0,
				len: 0,
				flags: 0,
				next: 0
			};
			vqsize
		]; // has to be 16 byte aligned
		let desc_table = desc_table.into_boxed_slice();
		// We need to be careful not to overflow the stack here. Use into_boxed_slice to get safe heap mem of desired sizes
		// init it as u16 to make casting to first to u16 elements easy. Need to divide by 2 compared to size in spec
		let avail_mem_box = vec![0 as u16; (6 + 2 * vqsize) >> 1].into_boxed_slice(); // has to be 2 byte aligned
		let used_mem_box = vec![0 as u16; (6 + 8 * vqsize) >> 1].into_boxed_slice(); // has to be 4 byte aligned

		// Leak memory so it wont get deallocated
		// TODO: create appropriate mem-owner-model. Pin these?
		let desc_table = alloc::boxed::Box::leak(desc_table);
		let avail_mem = alloc::boxed::Box::leak(avail_mem_box);
		let used_mem = alloc::boxed::Box::leak(used_mem_box);

		// try to use rust compilers ownership guarantees on virtq desc, by splitting array and putting owned values
		// which do not have destructors
		let mut desc_raw_wrappers: Vec<Box<virtq_desc_raw>> = Vec::new();
		for i in 0..vqsize {
			// "Recast" desc table entry into box, so we can freely move it around without worrying about the buffer
			// Since we have overwritten drop on virtq_desc_raw, this is safe, even if we never have allocated virtq_desc_raw with the global allocator!
			// TODO: is this actually true?
			let drw = unsafe { Box::from_raw(&mut desc_table[i] as *mut _) };
			desc_raw_wrappers.push(drw);
		}

		// 5.Optionally, if MSI-X capability is present and enabled on the device, select a vector to use to
		//   request interrupts triggered by virtqueue events. Write the MSI-X Table entry number corresponding to this
		//   vector into queue_msix_vector. Read queue_msix_vector:
		//   on success, previously written value is returned; on failure, NO_VECTOR value is returned.

		// Split buffers into usable structs:
		let (avail_flags, avail_mem) = avail_mem.split_first_mut().unwrap();
		let (avail_idx, avail_mem) = avail_mem.split_first_mut().unwrap();
		let (used_flags, used_mem) = used_mem.split_first_mut().unwrap();
		let (used_idx, used_mem) = used_mem.split_first_mut().unwrap();

		// Tell device about the guest-physical addresses of our queue structs:
		// TODO: cleanup pointer conversions (use &mut vq....?)
		common_cfg.queue_select = index;
		common_cfg.queue_desc = paging::virt_to_phys(desc_table.as_ptr() as usize) as u64;
		common_cfg.queue_avail = paging::virt_to_phys(avail_flags as *mut _ as usize) as u64;
		common_cfg.queue_used = paging::virt_to_phys(used_flags as *const _ as usize) as u64;
		common_cfg.queue_enable = 1;

		debug!(
			"desc 0x{:x}, avail 0x{:x}, used 0x{:x}",
			common_cfg.queue_desc, common_cfg.queue_avail, common_cfg.queue_used
		);

		let avail = VirtqAvail {
			flags: avail_flags,
			idx: avail_idx,
			ring: avail_mem,
			//rawmem: avail_mem_box,
		};
		let used = VirtqUsed {
			flags: used_flags,
			idx: used_idx,
			ring: unsafe { core::slice::from_raw_parts(used_mem.as_ptr() as *const _, vqsize) },
			//rawmem: used_mem_box,
			last_idx: 0,
		};
		let vq = Virtq::new(
			index,
			vqsize as u16,
			desc_raw_wrappers,
			avail,
			used,
			notify_cfg.get_notify_addr(common_cfg.queue_notify_off as u32),
		);

		return Some(vq);
	}

	fn notify_device(&mut self) {
		// 4.1.4.4.1 Device Requirements: Notification capability
		// virtio-fs does NOT offer VIRTIO_F_NOTIFICATION_DATA

		// 4.1.5.2 Available Buffer Notifications
		// When VIRTIO_F_NOTIFICATION_DATA has not been negotiated, the driver sends an available buffer notification
		// to the device by writing the 16-bit virtqueue index of this virtqueue to the Queue Notify address.
		trace!("Notifying device of updated virtqueue ({})...!", self.index);
		*self.queue_notify_address = self.index;
	}

	// Places dat in virtq, waits until buffer is used and response is in rsp_buf.
	pub fn send_blocking(&mut self, dat: &[&[u8]], rsp_buf: Option<&[&mut [u8]]>) {
		// 2.6.13 Supplying Buffers to The Device
		// The driver offers buffers to one of the device’s virtqueues as follows:

		// 1. The driver places the buffer into free descriptor(s) in the descriptor table, chaining as necessary (see 2.6.5 The Virtqueue Descriptor Table).

		// A buffer consists of zero or more device-readable physically-contiguous elements followed by zero or more physically-contiguous device-writable
		// elements (each has at least one element). This algorithm maps it into the descriptor table to form a descriptor chain:

		// 1. Get the next free descriptor table entry, d
		// Choose head=0, since we only do one req. TODO: get actual next free descr table entry
		let chainrc = self.virtq_desc.get_empty_chain();
		let mut chain = chainrc.borrow_mut();
		for dat in dat {
			self.virtq_desc.extend(&mut chain);
			let req = &mut chain.0.last_mut().unwrap().raw;

			// 2. Set d.addr to the physical address of the start of b
			req.addr = paging::virt_to_phys(dat.as_ptr() as usize) as u64;

			// 3. Set d.len to the length of b.
			req.len = dat.len() as u32; // TODO: better cast?

			// 4. If b is device-writable, set d.flags to VIRTQ_DESC_F_WRITE, otherwise 0.
			req.flags = 0;
			trace!("written out descriptor: {:?} @ {:p}", req, req);

			// 5. If there is a buffer element after this:
			//    a) Set d.next to the index of the next free descriptor element.
			//    b) Set the VIRTQ_DESC_F_NEXT bit in d.flags.
			// done by next extend call!
		}

		// if we want to receive a reply, we have to chain further descriptors, which declare VIRTQ_DESC_F_WRITE
		if let Some(rsp_buf) = rsp_buf {
			for dat in rsp_buf {
				self.virtq_desc.extend(&mut chain);
				let rsp = &mut chain.0.last_mut().unwrap().raw;
				rsp.addr = paging::virt_to_phys(dat.as_ptr() as usize) as u64;
				rsp.len = dat.len() as u32; // TODO: better cast?
				rsp.flags = VIRTQ_DESC_F_WRITE;
				trace!("written in descriptor: {:?} @ {:p}", rsp, rsp);
			}
		}

		trace!("Sending Descriptor chain {:?}", chain);

		// 2. The driver places the index of the head of the descriptor chain into the next ring entry of the available ring.
		let mut vqavail = self.avail.borrow_mut();
		let aind = (*vqavail.idx % self.vqsize) as usize;
		vqavail.ring[aind] = chain.0.first().unwrap().index;
		// TODO: add multiple descriptor chains at once?

		// 3. Steps 1 and 2 MAY be performed repeatedly if batching is possible.

		// 4. The driver performs a suitable memory barrier to ensure the device sees the updated descriptor table and available ring before the next step.
		// ????? TODO!

		// 5. The available idx is increased by the number of descriptor chain heads added to the available ring.
		// idx always increments, and wraps naturally at 65536:

		*vqavail.idx = vqavail.idx.wrapping_add(1);

		if *vqavail.idx == 0 {
			trace!("VirtQ index wrapped!");
		}

		// 6. The driver performs a suitable memory barrier to ensure that it updates the idx field before checking for notification suppression.
		// ????? TODO!

		// 7. The driver sends an available buffer notification to the device if such notifications are not suppressed.
		// 2.6.10.1 Driver Requirements: Available Buffer Notification Suppression
		// If the VIRTIO_F_EVENT_IDX feature bit is not negotiated:
		// - The driver MUST ignore the avail_event value.
		// - After the driver writes a descriptor index into the available ring:
		//     If flags is 1, the driver SHOULD NOT send a notification.
		//     If flags is 0, the driver MUST send a notification.
		let vqused = self.used.borrow();
		let should_notify = *vqused.flags == 0;
		drop(vqavail);
		drop(vqused);

		if should_notify {
			self.notify_device();
		}

		// wait until done (placed in used buffer)
		let mut vqused = self.used.borrow_mut();
		vqused.wait_until_done(&chain);

		// give chain back, so we can reuse the descriptors!
		drop(chain);
		self.virtq_desc.recycle_chain(chainrc)
	}
}

// Virtqueue descriptors: 16 bytes.
// These can chain together via "next".
#[repr(C)]
#[derive(Clone, Debug)]
pub struct virtq_desc_raw {
	// Address (guest-physical)
	// possibly optimize: https://rust-lang.github.io/unsafe-code-guidelines/layout/enums.html#layout-of-a-data-carrying-enums-without-a-repr-annotation
	// https://github.com/rust-lang/rust/pull/62514/files box will call destructor when removed.
	// BUT: we dont know buffer size, so T is not sized in Option<Box<T>> --> Box not simply a pointer?? [TODO: verify this! from https://github.com/rust-lang/unsafe-code-guidelines/issues/157#issuecomment-509016096]
	// nice, we have docs on this: https://doc.rust-lang.org/nightly/std/boxed/index.html#memory-layout
	// https://github.com/rust-lang/rust/issues/52976
	// Vec<T> is sized! but not just an array in memory.. --> impossible
	pub addr: u64,
	// Length
	pub len: u32,
	// The flags as indicated above (VIRTQ_DESC_F_*)
	pub flags: u16,
	// next field, if flags & NEXT
	// We chain unused descriptors via this, too
	pub next: u16,
}

impl Drop for virtq_desc_raw {
	fn drop(&mut self) {
		// TODO: what happens on shutdown etc?
		warn!("Dropping virtq_desc_raw, this is likely an error as of now! No memory will be deallocated!");
	}
}

// Single virtq descriptor. Pointer to raw descr, together with index
#[derive(Debug)]
struct VirtqDescriptor {
	index: u16,
	raw: Box<virtq_desc_raw>,
}

#[derive(Debug)]
struct VirtqDescriptorChain(Vec<VirtqDescriptor>);

// Two descriptor chains are equal, if memory address of vec is equal.
impl PartialEq for VirtqDescriptorChain {
	fn eq(&self, other: &Self) -> bool {
		&self.0 as *const _ == &other.0 as *const _
	}
}

struct VirtqDescriptors {
	// We need to guard against mem::forget. --> always store chains here?
	//    Do we? descriptors are in this file only, not external! -> We can ensure they are not mem::forgotten?
	//    still need to have them stored in this file somewhere though, cannot be owned by moved-out transfer object.
	//    So this is best solution?
	// free contains a single chain of all currently free descriptors.
	free: RefCell<VirtqDescriptorChain>,
	// a) We want to be able to use nonmutable reference to create new used chain
	// b) we want to return reference to descriptor chain, eg when creating new!
	// TODO: improve this type. there should be a better way to accomplish something similar.
	used_chains: RefCell<Vec<Rc<RefCell<VirtqDescriptorChain>>>>,
}

impl VirtqDescriptors {
	fn new(descr_raw: Vec<Box<virtq_desc_raw>>) -> Self {
		VirtqDescriptors {
			//descr_raw,
			free: RefCell::new(VirtqDescriptorChain(
				descr_raw
					.into_iter()
					.enumerate()
					.map(|(i, braw)| VirtqDescriptor {
						index: i as u16,
						raw: braw,
					})
					.rev()
					.collect(),
			)),
			used_chains: RefCell::new(Vec::new()),
		}
	}

	// Can't guarantee that the caller will pass back the chain to us, so never hand out complete ownership!
	fn get_empty_chain(&self) -> Rc<RefCell<VirtqDescriptorChain>> {
		// TODO: handle no-free case!
		//let mut free = self.free.borrow_mut();
		let mut used = self.used_chains.borrow_mut();
		let newchain = VirtqDescriptorChain(Vec::new() /*vec![free.0.pop().unwrap()]*/);
		let cell = Rc::new(RefCell::new(newchain));
		used.push(cell.clone());
		//Ref::map(, |mi| &mi.vec)
		//Ref::map(used.last().unwrap().borrow_mut(), |x| x)
		//used.last().unwrap().clone()
		cell
	}

	fn recycle_chain(&self, chain: Rc<RefCell<VirtqDescriptorChain>>) {
		let mut free = self.free.borrow_mut();
		let mut used = self.used_chains.borrow_mut();
		//info!("Free chain: {:?}", &free.0[free.0.len()-4..free.0.len()]);
		//info!("used chain: {:?}", &used);

		// Remove chain from used list
		// Two Rcs are equal if their inner values are equal, even if they are stored in different allocation.
		let index = used.iter().position(|c| *c == chain);
		if let Some(index) = index {
			used.remove(index);
		} else {
			warn!("Trying to remove chain from virtq which does not exist!");
			return;
		}
		free.0.append(&mut chain.borrow_mut().0);
		// chain is now empty! if anyone else still has a reference, he can't do harm
		// TODO: make test
		//info!("Free chain: {:?}", &free.0[free.0.len()-4..free.0.len()]);
		//info!("Used chain: {:?}", &used);
	}

	fn extend(&self, chain: &mut VirtqDescriptorChain) {
		// TODO: handle no-free case!
		let mut free = self.free.borrow_mut();
		let mut next = free.0.pop().unwrap();
		if !chain.0.is_empty() {
			let last = chain.0.last_mut().unwrap();
			last.raw.next = next.index;
			last.raw.flags |= VIRTQ_DESC_F_NEXT;
		}
		// Always make sure the chain is terminated properly
		next.raw.next = 0;
		next.raw.flags = 0;
		next.raw.len = 0;
		next.raw.addr = 0;
		chain.0.push(next);
	}
}

#[allow(dead_code)]
struct VirtqAvail<'a> {
	flags: &'a mut u16, // If VIRTIO_F_EVENT_IDX, set to 1 to maybe suppress interrupts
	idx: &'a mut u16,
	ring: &'a mut [u16],
	//rawmem: Box<[u16]>,
	// Only if VIRTIO_F_EVENT_IDX used_event: u16,
}

#[allow(dead_code)]
struct VirtqUsed<'a> {
	flags: &'a u16,
	idx: &'a u16,
	ring: &'a [virtq_used_elem],
	//rawmem: Box<[u16]>,
	last_idx: u16,
}

impl<'a> VirtqUsed<'a> {
	fn wait_until_done(&mut self, chain: &VirtqDescriptorChain) -> bool {
		// TODO: this might break if we have multiple running transfers at a time?
		while unsafe { core::ptr::read_volatile(self.idx) } == self.last_idx {}
		self.last_idx = *self.idx;

		let usedelem = self.ring[(self.last_idx.wrapping_sub(1) as usize) % self.ring.len()];

		trace!("Used Element: {:?}", usedelem);
		assert!(usedelem.id == chain.0.first().unwrap().index as u32);
		return true;

		// current version cannot fail.
		//false
	}
}

// u32 is used here for ids for padding reasons.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct virtq_used_elem {
	// Index of start of used descriptor chain.
	id: u32,
	// Total length of the descriptor chain which was written to.
	len: u32,
}

#[repr(C)]
#[derive(Debug)]
struct virtio_pci_cap {
	cap_vndr: u8,     /* Generic PCI field: PCI_CAP_ID_VNDR */
	cap_next: u8,     /* Generic PCI field: next ptr. */
	cap_len: u8,      /* Generic PCI field: capability length */
	cfg_type: u8,     /* Identifies the structure. */
	bar: u8,          /* Where to find it. */
	padding: [u8; 3], /* Pad to full dword. */
	offset: u32,      /* Offset within bar. */
	length: u32,      /* Length of the structure, in bytes. */
}

/// 4.1.4.4 Notification structure layout
/// The notification location is found using the VIRTIO_PCI_CAP_NOTIFY_CFG capability.
/// This capability is immediately followed by an additional field, notify_off_multiplier
#[repr(C)]
#[derive(Debug)]
pub struct virtio_pci_notify_cap {
	/* About the whole device. */
	cap: virtio_pci_cap,
	notify_off_multiplier: u32, /* Multiplier for queue_notify_off. */
}

#[repr(C)]
#[derive(Debug)]
pub struct virtio_pci_common_cfg {
	/* About the whole device. */
	pub device_feature_select: u32, /* read-write */
	pub device_feature: u32,        /* read-only for driver */
	pub driver_feature_select: u32, /* read-write */
	pub driver_feature: u32,        /* read-write */
	pub msix_config: u16,           /* read-write */
	pub num_queues: u16,            /* read-only for driver */
	pub device_status: u8,          /* read-write */
	pub config_generation: u8,      /* read-only for driver */

	/* About a specific virtqueue. */
	pub queue_select: u16,      /* read-write */
	pub queue_size: u16,        /* read-write, power of 2, or 0. */
	pub queue_msix_vector: u16, /* read-write */
	pub queue_enable: u16,      /* read-write */
	pub queue_notify_off: u16,  /* read-only for driver */
	pub queue_desc: u64,        /* read-write */
	pub queue_avail: u64,       /* read-write */
	pub queue_used: u64,        /* read-write */
}

#[derive(Debug)]
pub struct VirtioNotification {
	pub notification_ptr: *mut u16,
	pub notify_off_multiplier: u32,
}

impl VirtioNotification {
	pub fn get_notify_addr(&self, queue_notify_off: u32) -> &'static mut u16 {
		// divide by 2 since notification_ptr is a u16 pointer but we have byte offset
		let addr = unsafe {
			&mut *self
				.notification_ptr
				.offset((queue_notify_off * self.notify_off_multiplier) as isize / 2)
		};
		debug!(
			"Queue notify address parts: {:p} {} {} {:p}",
			self.notification_ptr, queue_notify_off, self.notify_off_multiplier, addr
		);
		addr
	}
}

/// Scans pci-capabilities for a virtio-capability of type virtiocaptype.
/// When found, maps it into memory and returns virtual address, else None
pub fn map_virtiocap(
	bus: u8,
	device: u8,
	adapter: &PciAdapter,
	caplist: u32,
	virtiocaptype: u32,
) -> Option<(usize, u32)> {
	let mut nextcaplist = caplist;
	if nextcaplist < 0x40 {
		error!(
			"Caplist inside header! Offset: 0x{:x}, Aborting",
			nextcaplist
		);
		return None;
	}

	// Debug dump all
	/*for x in (0..255).step_by(4) {
			debug!("{:02x}: {:08x}", x, pci::read_config(bus, device, x));
	}*/

	// Loop through capabilities until vendor (virtio) defined one is found
	let virtiocapoffset = loop {
		if nextcaplist == 0 || nextcaplist < 0x40 {
			error!("Next caplist invalid, and still not found the wanted virtio cap, aborting!");
			return None;
		}
		let captypeword = pci::read_config(bus, device, nextcaplist);
		debug!(
			"Read cap at offset 0x{:x}: captype 0x{:x}",
			nextcaplist, captypeword
		);
		let captype = captypeword & 0xFF; // pci cap type
		if captype == pci::PCI_CAP_ID_VNDR {
			// we are vendor defined, with virtio vendor --> we can check for virtio cap type
			debug!("found vendor, virtio type: {}", (captypeword >> 24) & 0xFF);
			if (captypeword >> 24) & 0xFF == virtiocaptype {
				break nextcaplist;
			}
		}
		nextcaplist = (captypeword >> 8) & 0xFF; // pci cap next ptr
	};
	// Since we have verified caplistoffset to be virtio_pci_cap common config, read fields.
	// TODO: cleanup 'hacky' type conversions
	let bar: usize = (pci::read_config(bus, device, virtiocapoffset + 4) & 0xFF) as usize; // get offset_of!(virtio_pci_cap, bar)
	let offset: usize = pci::read_config(bus, device, virtiocapoffset + 8) as usize; // get offset_of!(virtio_pci_cap, offset)
	let length: usize = pci::read_config(bus, device, virtiocapoffset + 12) as usize; // get offset_of!(virtio_pci_cap, length)
	debug!(
		"Found virtio config bar as 0x{:x}, offset 0x{:x}, length 0x{:x}",
		bar, offset, length
	);

	if (adapter.base_sizes[bar] as usize) < offset + length {
		error!(
			"virtio config struct does not fit in bar! Aborting! 0x{:x} < 0x{:x}",
			adapter.base_sizes[bar],
			offset + length
		);
		return None;
	}

	// base_addresses from bar are IOBASE?
	// TODO: do proper memmapped bars in pci.rs
	let barword = pci::read_config(bus, device, pci::PCI_BAR0_REGISTER + ((bar as u32) << 2));
	debug!("Found bar{} as 0x{:x}", bar, barword);
	assert!(barword & 1 == 0, "Not an memory mapped bar!");

	let bartype = (barword >> 1) & 0b11;
	assert!(bartype == 2, "Not a 64 bit bar!");

	let prefetchable = (barword >> 3) & 1;
	assert!(prefetchable == 1, "Bar not prefetchable, but 64 bit!");

	let barwordhigh = pci::read_config(
		bus,
		device,
		pci::PCI_BAR0_REGISTER + (((bar + 1) as u32) << 2),
	);
	//let barbase = barwordhigh << 33; // creates general protection fault... only when shifting by >=32 though..
	let barbase: usize = ((barwordhigh as usize) << 32) + (barword & 0xFFFF_FFF0) as usize;

	debug!(
		"Mapping bar {} at 0x{:x} with length 0x{:x}",
		bar, barbase, length
	);
	// corrosponding setup in eg Qemu @ https://github.com/qemu/qemu/blob/master/hw/virtio/virtio-pci.c#L1590 (virtio_pci_device_plugged)
	// map 1 page (0x1000?) TODO: map "length"!
	// for virtio-fs we are "lucky" and each cap is exactly one page (contiguous, but we dont care, map each on its own)
	let membase = barbase as *mut u8;
	let capbase = unsafe { membase.offset(offset as isize) as usize };
	let virtualcapaddr = virtualmem::allocate(BasePageSize::SIZE).unwrap();
	let mut flags = paging::PageTableEntryFlags::empty();
	flags.device().writable().execute_disable();
	paging::map::<BasePageSize>(virtualcapaddr, capbase, 1, flags);

	/*
		let mut slice = unsafe {
				core::slice::from_raw_parts_mut(membase.offset(offset as isize), length)
		};
		info!("{:?}", slice);
	*/
	if virtiocaptype == VIRTIO_PCI_CAP_NOTIFY_CFG {
		let notify_off_multiplier: u32 = pci::read_config(bus, device, virtiocapoffset + 16); // get offset_of!(virtio_pci_notify_cap, notify_off_multiplier)
		Some((virtualcapaddr, notify_off_multiplier))
	} else {
		Some((virtualcapaddr, 0))
	}
}

pub fn init_virtio_device(adapter: pci::PciAdapter) {
	// TODO: 2.3.1: Loop until get_config_generation static, since it might change mid-read

	if adapter.device_id <= 0x103F {
		// Legacy device, skip
		info!("Legacy Virtio device, skipping!");
		return;
	}
	let virtio_device_id = adapter.device_id - 0x1040;

	let drv = match virtio_device_id {
		0x1a => {
			info!("Found Virtio-FS device!");
			// TODO: proper error handling on driver creation fail
			virtio_fs::create_virtiofs_driver(adapter).unwrap()
		}
		_ => {
			info!("Virtio device is NOT virtio-fs device, skipping!");
			return;
		}
	};

	// Install interrupt handler
	// TODO: get irqnumber from pci, don't hardcode 11.
	irq_install_handler(11, virtio_irqhandler as usize);

	pci::register_driver(PciDriver::VirtioFs(drv));
}

#[cfg(target_arch = "x86_64")]
extern "x86-interrupt" fn virtio_irqhandler(_stack_frame: &mut ExceptionStackFrame) {
	debug!("Receive virtio interrupt");
	apic::eoi();
	core_scheduler().scheduler();
}
