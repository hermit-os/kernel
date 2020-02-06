use self::consts::*;
use alloc::boxed::Box;
use alloc::rc::Rc;
use alloc::vec::Vec;
use arch::x86_64::kernel::pci;
use arch::x86_64::kernel::pci::{PciAdapter, PciDriver};
use arch::x86_64::mm::paging::{BasePageSize, PageSize};
use arch::x86_64::mm::{paging, virtualmem};
use core::cell::RefCell;
use core::{fmt, u32, u8};

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

pub struct VirtiofsDriver<'a> {
	common_cfg: &'a mut virtio_pci_common_cfg,
	device_cfg: &'a virtio_fs_config,
	notify_cfg: VirtioNotification,
	vqueues: Option<Vec<Virtq<'a>>>,
}

impl<'a> fmt::Debug for VirtiofsDriver<'a> {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "VirtiofsDriver {{ ")?;
		write!(f, "common_cfg: {:?}, ", self.common_cfg)?;
		write!(f, "device_cfg: {:?}, ", self.device_cfg)?;
		write!(f, "nofity_cfg: {:?}, ", self.device_cfg)?;
		match &self.vqueues {
			None => write!(f, "Uninitialized VQs")?,
			Some(vqs) => write!(f, "Initialized {} VQs", vqs.len())?,
		}
		write!(f, "}}")
	}
}

impl VirtiofsDriver<'_> {
	pub fn init_vqs(&mut self) {
		let common_cfg = &mut self.common_cfg;
		let device_cfg = &self.device_cfg;
		let notify_cfg = &mut self.notify_cfg;

		// 4.1.5.1.3 Virtqueueu configuration
		// see https://elixir.bootlin.com/linux/latest/ident/virtio_fs_setup_vqs for example
		info!("Setting up virtqueues...");

		if device_cfg.num_request_queues == 0 {
			error!("0 request queues requested from device. Aborting!");
			return;
		}
		// 1 highprio queue, and n normal request queues
		let vqnum = device_cfg.num_request_queues + 1;
		let mut vqueues = Vec::<Virtq>::new();

		// create the queues and tell device about them
		for i in 0..vqnum as u16 {
			// 1.Write the virtqueue index to queue_select.
			common_cfg.queue_select = i;

			// 2.Read the virtqueue size from queue_size. This controls how big the virtqueue is (see 2.4 Virtqueues).
			//   If this field is 0, the virtqueue does not exist.
			let vqsize = common_cfg.queue_size as usize;
			if vqsize == 0 || vqsize > 32768 {
				return;
			}
			info!("Initializing virtqueue {}, of size {}", i, vqsize);

			// 3.Optionally, select a smaller virtqueue size and write it to queue_size.

			// 4.Allocate and zero Descriptor Table, Available and Used rings for the virtqueue in contiguous physical memory.
			// TODO: is this contiguous memory?
			// TODO: (from 2.6.13.1 Placing Buffers Into The Descriptor Table):
			//   In practice, d.next is usually used to chain free descriptors,
			//   and a separate count kept to check there are enough free descriptors before beginning the mappings.
			let desc_table = vec![
				virtq_desc {
					addr: 0,
					len: 0,
					flags: 0,
					next: 0
				};
				vqsize
			]; // has to be 16 byte aligned
   // We need to be careful not to overflow the stack here. Use into_boxed_slice to get safe heap mem of desired sizes
   // init it as u16 to make casting to first to u16 elements easy. Need to divide by 2 compared to size in spec
			let avail_mem_box = vec![0 as u16; (6 + 2 * vqsize) >> 1].into_boxed_slice();
			let used_mem_box = vec![0 as u16; (6 + 8 * vqsize) >> 1].into_boxed_slice();

			// Leak memory so it wont get deallocated
			// TODO: create appropriate mem-owner-model
			let avail_mem = alloc::boxed::Box::leak(avail_mem_box);
			let used_mem = alloc::boxed::Box::leak(used_mem_box);

			// 5.Optionally, if MSI-X capability is present and enabled on the device, select a vector to use to
			//   request interrupts triggered by virtqueue events. Write the MSI-X Table entry number corresponding to this
			//   vector into queue_msix_vector. Read queue_msix_vector:
			//   on success, previously written value is returned; on failure, NO_VECTOR value is returned.

			// WHERE IS THIS SPEC FROM? IGNORE FOR NOW
			// The driver notifies the device by writing the 16-bit virtqueue index of this virtqueue to the Queue Notify address.
			// See 4.1.4.4 for how to calculate this address

			// Split buffers into usable structs:
			let (avail_flags, avail_mem) = avail_mem.split_first_mut().unwrap();
			let (avail_idx, avail_mem) = avail_mem.split_first_mut().unwrap();
			let (used_flags, used_mem) = used_mem.split_first_mut().unwrap();
			let (used_idx, used_mem) = used_mem.split_first_mut().unwrap();

			// Tell device about the guest-physical addresses of our queue structs:
			// TODO: cleanup pointer conversions (use &mut vq....?)
			common_cfg.queue_select = i;
			common_cfg.queue_desc = paging::virt_to_phys(desc_table.as_ptr() as usize) as u64;
			common_cfg.queue_avail = paging::virt_to_phys(avail_flags as *mut _ as usize) as u64;
			common_cfg.queue_used = paging::virt_to_phys(used_flags as *const _ as usize) as u64;
			common_cfg.queue_enable = 1;

			info!(
				"desc 0x{:x}, avail 0x{:x}, used 0x{:x}",
				common_cfg.queue_desc, common_cfg.queue_avail, common_cfg.queue_used
			);

			let avail = virtq_avail {
				flags: avail_flags,
				idx: avail_idx,
				ring: avail_mem,
				//rawmem: avail_mem_box,
			}; // has to be 2 byte aligned
			let used = virtq_used {
				flags: used_flags,
				idx: used_idx,
				ring: unsafe { core::slice::from_raw_parts(used_mem.as_ptr() as *const _, vqsize) },
				//rawmem: used_mem_box,
			}; // has to be 4 byte aligned
			let vq = Virtq {
				index: i,
				num: vqsize as u16,
				virtq_desc: desc_table,
				avail: Rc::new(RefCell::new(avail)),
				used: Rc::new(RefCell::new(used)),
				queue_notify_address: notify_cfg
					.get_notify_addr(common_cfg.queue_notify_off as u32),
			};

			vqueues.push(vq);
		}

		self.vqueues = Some(vqueues);
	}

	pub fn negotiate_features(&mut self) {
		let common_cfg = &mut self.common_cfg;
		// Linux kernel reads 2x32 featurebits: https://elixir.bootlin.com/linux/latest/ident/vp_get_features
		common_cfg.device_feature_select = 0;
		let mut device_features: u64 = common_cfg.device_feature as u64;
		common_cfg.device_feature_select = 1;
		device_features |= (common_cfg.device_feature as u64) << 32;

		if device_features & VIRTIO_F_RING_INDIRECT_DESC != 0 {
			info!("Device offers feature VIRTIO_F_RING_INDIRECT_DESC, ignoring");
		}
		if device_features & VIRTIO_F_RING_EVENT_IDX != 0 {
			info!("Device offers feature VIRTIO_F_RING_EVENT_IDX, ignoring");
		}
		if device_features & VIRTIO_F_VERSION_1 != 0 {
			info!("Device offers feature VIRTIO_F_VERSION_1, accepting.");
			common_cfg.driver_feature_select = 1;
			common_cfg.driver_feature = (VIRTIO_F_VERSION_1 >> 32) as u32;
		}
		if device_features
			& !(VIRTIO_F_RING_INDIRECT_DESC | VIRTIO_F_RING_EVENT_IDX | VIRTIO_F_VERSION_1)
			!= 0
		{
			info!(
				"Device offers unknown feature bits: {:064b}.",
				device_features
			);
		}
		// There are no virtio-fs specific featurebits yet.
		// TODO: actually check features
		// currently provided features of virtio-fs:
		// 0000000000000000000000000000000100110000000000000000000000000000
		// only accept VIRTIO_F_VERSION_1 for now.

		/*
		// on failure:
		common_cfg.device_status |= 128;
		return ERROR;
		*/
	}

	/// 3.1 VirtIO Device Initialization
	pub fn init(&mut self) {
		// 1.Reset the device.
		self.common_cfg.device_status = 0;

		// 2.Set the ACKNOWLEDGE status bit: the guest OS has notice the device.
		self.common_cfg.device_status |= 1;

		// 3.Set the DRIVER status bit: the guest OS knows how to drive the device.
		self.common_cfg.device_status |= 2;

		// 4.Read device feature bits, and write the subset of feature bits understood by the OS and driver to the device.
		//   During this step the driver MAY read (but MUST NOT write) the device-specific configuration fields to check
		//   that it can support the device before accepting it.
		self.negotiate_features();

		// 5.Set the FEATURES_OK status bit. The driver MUST NOT accept new feature bits after this step.
		self.common_cfg.device_status |= 8;

		// 6.Re-read device status to ensure the FEATURES_OK bit is still set:
		//   otherwise, the device does not support our subset of features and the device is unusable.
		if self.common_cfg.device_status & 8 == 0 {
			error!("Device unset FEATURES_OK, aborting!");
			return;
		}

		// 7.Perform device-specific setup, including discovery of virtqueues for the device, optional per-bus setup,
		//   reading and possibly writing the device’s virtio configuration space, and population of virtqueues.
		self.init_vqs();

		// 8.Set the DRIVER_OK status bit. At this point the device is “live”.
		self.common_cfg.device_status |= 4;
	}

	pub fn send_hello(&mut self) {
		// Setup virtio-fs (5.11 in virtio spec @ https://stefanha.github.io/virtio/virtio-fs.html#x1-41500011)
		// 5.11.5 Device Initialization
		// On initialization the driver first discovers the device’s virtqueues.
		// The FUSE session is started by sending a FUSE_INIT request as defined by the FUSE protocol on one request virtqueue.
		// All virtqueues provide access to the same FUSE session and therefore only one FUSE_INIT request is required
		// regardless of the number of available virtqueues.

		// 5.11.6 Device Operation
		// TODO: send a simple getdents as test
		// Send FUSE_INIT
		// example, see https://elixir.bootlin.com/linux/latest/source/fs/fuse/inode.c#L973 (fuse_send_init)
		// https://github.com/torvalds/linux/blob/76f6777c9cc04efe8036b1d2aa76e618c1631cc6/fs/fuse/dev.c#L1190 <<- max_write
		if let Some(ref mut vqueues) = self.vqueues {
			let outbuf = [0; 128];
			vqueues[1].insert_into_queue(
				&[
					// fuse_in_header
					96, 0, 0,
					0, // pub len: u32, // 96 for all bytes!. Yet still returns: "elem 0 too short for out_header" "elem 0 no reply sent"
					26, 0, 0, 0, // pub opcode: u32,
					1, 0, 0, 0, 0, 0, 0, 0, // pub unique: u64,
					1, 0, 0, 0, 0, 0, 0, 0, // pub nodeid: u64,
					0, 0, 0, 0, // pub uid: u32,
					0, 0, 0, 0, // pub gid: u32,
					1, 0, 0, 0, // pub pid: u32,
					0, 0, 0, 0, // pub padding: u32,
					// fuse_init_in
					7, 0, 0, 0, // major
					31, 0, 0, 0, // minor
					0, 0, 0, 0, // max_readahead
					0, 0, 0, 0, // flags
					// fuse_out_header
					0, 0, 0, 0, // pub len: u32,
					0, 0, 0, 0, // pub error: i32,
					0, 0, 0, 0, 0, 0, 0, 0, // pub unique: u64,
					// fuse_init_out
					0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
				],
				Some(&outbuf),
			);
			// TODO: Answer is already here. This is not guaranteed by any spec, we should wait until it appears in used ring!
			info!("{:?}", &outbuf[..]);
		}
	}
}

struct Virtq<'a> {
	index: u16, // Index of vq in common config
	num: u16,   // Elements in ring/descrs
	// The actial descriptors (16 bytes each)
	virtq_desc: Vec<virtq_desc>,
	// A ring of available descriptor heads with free-running index
	avail: Rc<RefCell<virtq_avail<'a>>>,
	// A ring of used descriptor heads with free-running index
	used: Rc<RefCell<virtq_used<'a>>>,
	// Address where queue index is written to on notify
	queue_notify_address: &'a mut u16,
}

impl Virtq<'_> {
	fn notify_device(&mut self) {
		// 4.1.4.4.1 Device Requirements: Notification capability
		// virtio-fs does NOT offer VIRTIO_F_NOTIFICATION_DATA

		// 4.1.5.2 Available Buffer Notifications
		// When VIRTIO_F_NOTIFICATION_DATA has not been negotiated, the driver sends an available buffer notification
		// to the device by writing the 16-bit virtqueue index of this virtqueue to the Queue Notify address.
		info!("Notifying device of updated virtqueue ({})...!", self.index);
		*self.queue_notify_address = self.index;
	}

	fn insert_into_queue(&mut self, dat: &[u8], rsp_buf: Option<&[u8]>) {
		// 2.6.13 Supplying Buffers to The Device
		// The driver offers buffers to one of the device’s virtqueues as follows:

		// 1. The driver places the buffer into free descriptor(s) in the descriptor table, chaining as necessary (see 2.6.5 The Virtqueue Descriptor Table).

		// A buffer consists of zero or more device-readable physically-contiguous elements followed by zero or more physically-contiguous device-writable
		// elements (each has at least one element). This algorithm maps it into the descriptor table to form a descriptor chain:

		// 1. Get the next free descriptor table entry, d
		// Choose head=0, since we only do one req. TODO: get actual next free descr table entry
		let head: u16 = 0 % self.num;
		let req = &mut self.virtq_desc[head as usize];

		// 2. Set d.addr to the physical address of the start of b
		req.addr = paging::virt_to_phys(dat.as_ptr() as usize) as u64;

		// 3. Set d.len to the length of b.
		req.len = dat.len() as u32; // TODO: better cast?
		info!("Transfering buffer of len {}", req.len);

		// 4. If b is device-writable, set d.flags to VIRTQ_DESC_F_WRITE, otherwise 0.
		req.flags = 0;

		// 5. If there is a buffer element after this:
		//    a) Set d.next to the index of the next free descriptor element.
		//    b) Set the VIRTQ_DESC_F_NEXT bit in d.flags.
		// if we want to receive a reply, we have to chain to another descriptor, which declares VIRTQ_DESC_F_WRITE
		if let Some(rsp_buf) = rsp_buf {
			let next = head + 1;

			req.next = next;
			req.flags = VIRTQ_DESC_F_NEXT;
			info!("written descriptor: {:?} @ {:p}", req, req);
			drop(req);

			let rsp = &mut self.virtq_desc[next as usize];
			rsp.addr = paging::virt_to_phys(rsp_buf.as_ptr() as usize) as u64;
			rsp.len = rsp_buf.len() as u32; // TODO: better cast?
			rsp.flags = VIRTQ_DESC_F_WRITE;
			rsp.next = 0;
		} else {
			req.next = 0;
			info!("written descriptor: {:?} @ {:p}", req, req);
		}

		// 2. The driver places the index of the head of the descriptor chain into the next ring entry of the available ring.
		let mut avail = self.avail.borrow_mut();
		let aind = (*avail.idx % self.num) as usize;
		avail.ring[aind] = head;
		// TODO: add multiple descriptor chains at once?

		// 3. Steps 1 and 2 MAY be performed repeatedly if batching is possible.

		// 4. The driver performs a suitable memory barrier to ensure the device sees the updated descriptor table and available ring before the next step.
		// ????? TODO!

		// 5. The available idx is increased by the number of descriptor chain heads added to the available ring.
		// idx always increments, and wraps naturally at 65536:
		*avail.idx += 1;

		// 6. The driver performs a suitable memory barrier to ensure that it updates the idx field before checking for notification suppression.
		// ????? TODO!

		// 7. The driver sends an available buffer notification to the device if such notifications are not suppressed.
		// 2.6.10.1 Driver Requirements: Available Buffer Notification Suppression
		// If the VIRTIO_F_EVENT_IDX feature bit is not negotiated:
		// - The driver MUST ignore the avail_event value.
		// - After the driver writes a descriptor index into the available ring:
		//     If flags is 1, the driver SHOULD NOT send a notification.
		//     If flags is 0, the driver MUST send a notification.
		let used = self.used.borrow();
		let should_notify = *used.flags == 0;
		drop(avail);
		drop(used);
		if should_notify {
			self.notify_device();
		}
	}
}

// Virtqueue descriptors: 16 bytes.
// These can chain together via "next".
#[repr(C)]
#[derive(Clone, Debug)]
struct virtq_desc {
	// Address (guest-physical)
	// possibly optimize: https://rust-lang.github.io/unsafe-code-guidelines/layout/enums.html#layout-of-a-data-carrying-enums-without-a-repr-annotation
	// https://github.com/rust-lang/rust/pull/62514/files box will call destructor when removed.
	// BUT: we dont know buffer size, so T is not sized in Option<Box<T>> --> Box not simply a pointer?? [TODO: verify this! from https://github.com/rust-lang/unsafe-code-guidelines/issues/157#issuecomment-509016096]
	// nice, we have docs on this: https://doc.rust-lang.org/nightly/std/boxed/index.html#memory-layout
	// https://github.com/rust-lang/rust/issues/52976
	// Vec<T> is sized! but not just an array in memory.. --> impossible
	addr: u64,
	// Length
	len: u32,
	// The flags as indicated above (VIRTQ_DESC_F_*)
	flags: u16,
	// next field, if flags & NEXT
	// We chain unused descriptors via this, too
	next: u16,
}

#[repr(C)]
struct virtq_avail<'a> {
	flags: &'a mut u16, // If VIRTIO_F_EVENT_IDX, set to 1 to maybe suppress interrupts
	idx: &'a mut u16,
	ring: &'a mut [u16],
	//rawmem: Box<[u16]>,
	// Only if VIRTIO_F_EVENT_IDX used_event: u16,
}

#[repr(C)]
struct virtq_used<'a> {
	flags: &'a u16,
	idx: &'a u16,
	ring: &'a [virtq_used_elem],
	//rawmem: Box<[u16]>,
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
struct virtio_pci_notify_cap {
	/* About the whole device. */
	cap: virtio_pci_cap,
	notify_off_multiplier: u32, /* Multiplier for queue_notify_off. */
}

#[repr(C)]
#[derive(Debug)]
struct virtio_pci_common_cfg {
	/* About the whole device. */
	device_feature_select: u32, /* read-write */
	device_feature: u32,        /* read-only for driver */
	driver_feature_select: u32, /* read-write */
	driver_feature: u32,        /* read-write */
	msix_config: u16,           /* read-write */
	num_queues: u16,            /* read-only for driver */
	device_status: u8,          /* read-write */
	config_generation: u8,      /* read-only for driver */

	/* About a specific virtqueue. */
	queue_select: u16,      /* read-write */
	queue_size: u16,        /* read-write, power of 2, or 0. */
	queue_msix_vector: u16, /* read-write */
	queue_enable: u16,      /* read-write */
	queue_notify_off: u16,  /* read-only for driver */
	queue_desc: u64,        /* read-write */
	queue_avail: u64,       /* read-write */
	queue_used: u64,        /* read-write */
}

#[derive(Debug)]
struct VirtioNotification {
	notification_ptr: *mut u16,
	notify_off_multiplier: u32,
}

impl VirtioNotification {
	pub fn get_notify_addr(&self, queue_notify_off: u32) -> &'static mut u16 {
		// divide by 2 since notification_ptr is a u16 pointer but we have byte offset
		let addr = unsafe {
			&mut *self
				.notification_ptr
				.offset((queue_notify_off * self.notify_off_multiplier) as isize / 2)
		};
		info!(
			"Queue notify address parts: {:p} {} {} {:p}",
			self.notification_ptr, queue_notify_off, self.notify_off_multiplier, addr
		);
		addr
	}
}

#[repr(C)]
struct virtio_fs_config {
	/* Filesystem name (UTF-8, not NUL-terminated, padded with NULs) */
	tag: [u8; 36],
	/* Number of request queues */
	num_request_queues: u32,
}

impl fmt::Debug for virtio_fs_config {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(
			f,
			"virtio_fs_config {{ tag: '{}', num_request_queues: {} }}",
			core::str::from_utf8(&self.tag).unwrap(),
			self.num_request_queues
		)
	}
}

/// Scans pci-capabilities for a virtio-capability of type virtiocaptype.
/// When found, maps it into memory and returns virtual address, else None
fn map_virtiocap(
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
			info!("{:02x}: {:08x}", x, pci::read_config(bus, device, x));
	}*/

	// Loop through capabilities until vendor (virtio) defined one is found
	let virtiocapoffset = loop {
		if nextcaplist == 0 || nextcaplist < 0x40 {
			error!("Next caplist invalid, and still not found the wanted virtio cap, aborting!");
			return None;
		}
		let captypeword = pci::read_config(bus, device, nextcaplist);
		info!(
			"Read cap at offset 0x{:x}: captype 0x{:x}",
			nextcaplist, captypeword
		);
		let captype = captypeword & 0xFF; // pci cap type
		if captype == pci::PCI_CAP_ID_VNDR {
			// we are vendor defined, with virtio vendor --> we can check for virtio cap type
			info!("found vendor, virtio type: {}", (captypeword >> 24) & 0xFF);
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
	info!(
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
	// TODO: fix this hack. bar is assumed to be mem-mapped
	let barword = pci::read_config(bus, device, pci::PCI_BAR0_REGISTER + ((bar as u32) << 2));
	info!("Found bar{} as 0x{:x}", bar, barword);
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

	info!(
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

pub fn create_virtio_driver(adapter: pci::PciAdapter) -> Option<Box<VirtiofsDriver<'static>>> {
	// Scan capabilities to get common config, which we need to reset the device and get basic info.
	// also see https://elixir.bootlin.com/linux/latest/source/drivers/virtio/virtio_pci_modern.c#L581 (virtio_pci_modern_probe)
	// Read status register
	let bus = adapter.bus;
	let device = adapter.device;
	let status = pci::read_config(bus, device, pci::PCI_COMMAND_REGISTER) >> 16;

	// non-legacy virtio device always specifies capability list, so it can tell us in which bar we find the virtio-config-space
	if status & pci::PCI_STATUS_CAPABILITIES_LIST == 0 {
		error!("Found virtio device without capability list. Likely legacy-device! Aborting.");
		return None;
	}

	// Get pointer to capability list
	let caplist = pci::read_config(bus, device, pci::PCI_CAPABILITY_LIST_REGISTER) & 0xFF;

	// get common config mapped, cast to virtio_pci_common_cfg
	let common_cfg = match map_virtiocap(bus, device, &adapter, caplist, VIRTIO_PCI_CAP_COMMON_CFG)
	{
		Some((cap_common_raw, _)) => unsafe {
			&mut *(cap_common_raw as *mut virtio_pci_common_cfg)
		},
		None => {
			error!("Could not find VIRTIO_PCI_CAP_COMMON_CFG. Aborting!");
			return None;
		}
	};
	// get device config mapped, cast to virtio_fs_config
	let device_cfg = match map_virtiocap(bus, device, &adapter, caplist, VIRTIO_PCI_CAP_DEVICE_CFG)
	{
		Some((cap_device_raw, _)) => unsafe { &mut *(cap_device_raw as *mut virtio_fs_config) },
		None => {
			error!("Could not find VIRTIO_PCI_CAP_DEVICE_CFG. Aborting!");
			return None;
		}
	};
	// get device notifications mapped
	let (notification_ptr, notify_off_multiplier) =
		match map_virtiocap(bus, device, &adapter, caplist, VIRTIO_PCI_CAP_NOTIFY_CFG) {
			Some((cap_notification_raw, notify_off_multiplier)) => {
				((
					cap_notification_raw as *mut u16, // unsafe { core::slice::from_raw_parts_mut::<u16>(...)}
					notify_off_multiplier,
				))
			}
			None => {
				error!("Could not find VIRTIO_PCI_CAP_NOTIFY_CFG. Aborting!");
				return None;
			}
		};
	let notify_cfg = VirtioNotification {
		notification_ptr,
		notify_off_multiplier,
	};

	// TODO: also load the other 2 cap types (?).

	// Instanciate driver on heap, so it outlives this function
	let mut drv = Box::new(VirtiofsDriver {
		common_cfg,
		device_cfg,
		notify_cfg,
		vqueues: None,
	});

	info!("Driver before init: {:?}", drv);
	drv.init();
	info!("Driver after init: {:?}", drv);

	Some(drv)
}

pub fn init_virtio_device(adapter: pci::PciAdapter) {
	// TODO: 2.3.1: Loop until get_config_generation static, since it might change mid-read

	if adapter.device_id <= 0x103F {
		// Legacy device, skip
		return;
	}
	let virtio_device_id = adapter.device_id - 0x1040;

	if virtio_device_id == 0x1a {
		info!("Found Virtio-FS device!");
	} else {
		// Not a virtio-fs device
		info!("Virtio device is NOT virtio-fs device, skipping!");
		return;
	}

	// TODO: proper error handling on driver creation fail
	let mut drv = create_virtio_driver(adapter).unwrap();

	drv.send_hello();

	pci::register_driver(PciDriver::VirtioFs(drv));
}
