//! A virtio-net driver.
//!
//! For details on the device, see [Network Device].
//! For details on the Rust definitions, see [`virtio::net`].
//!
//! [Network Device]: https://docs.oasis-open.org/virtio/virtio/v1.2/cs01/virtio-v1.2-cs01.html#x1-2170001

cfg_if::cfg_if! {
	if #[cfg(feature = "pci")] {
		mod pci;
	} else {
		mod mmio;
	}
}

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::mem::{ManuallyDrop, MaybeUninit, transmute};
use core::str::FromStr;

use smallvec::SmallVec;
use smoltcp::phy::{Checksum, ChecksumCapabilities, DeviceCapabilities};
use smoltcp::wire::{
	ETHERNET_HEADER_LEN, EthernetFrame, IpAddress, IpProtocol, Ipv4Packet, Ipv6Packet, TcpPacket,
	UdpPacket,
};
use virtio::DeviceConfigSpace;
use virtio::net::{ConfigVolatileFieldAccess, Hdr, HdrF};
use volatile::VolatileRef;
use volatile::access::ReadOnly;

use self::constants::MAX_NUM_VQ;
use self::error::VirtioNetError;
use crate::config::VIRTIO_MAX_QUEUE_SIZE;
use crate::drivers::net::virtio::constants::BUFF_PER_PACKET;
use crate::drivers::net::{NetworkDriver, mtu};
use crate::drivers::virtio::ControlRegisters;
#[cfg(not(feature = "pci"))]
use crate::drivers::virtio::transport::mmio::{ComCfg, IsrStatus, NotifCfg};
#[cfg(feature = "pci")]
use crate::drivers::virtio::transport::pci::{ComCfg, IsrStatus, NotifCfg};
use crate::drivers::virtio::virtqueue::packed::PackedVq;
use crate::drivers::virtio::virtqueue::split::SplitVq;
use crate::drivers::virtio::virtqueue::{
	AvailBufferToken, BufferElem, BufferType, UsedBufferToken, VirtQueue, Virtq,
};
use crate::drivers::{Driver, InterruptLine};
use crate::mm::device_alloc::DeviceAlloc;

/// A wrapper struct for the raw configuration structure.
/// Handling the right access to fields, as some are read-only
/// for the driver.
pub(crate) struct NetDevCfg {
	pub raw: VolatileRef<'static, virtio::net::Config, ReadOnly>,
	pub dev_id: u16,
	pub features: virtio::net::F,
}

fn determine_mtu(dev_cfg: &NetDevCfg) -> u16 {
	// If VIRTIO_NET_F_MTU is negotiated, "the driver uses mtu as the maximum MTU value"
	// (VirtIO specification, 5.1.3, "Feature bits")
	if dev_cfg.features.contains(virtio::net::F::MTU) {
		dev_cfg.raw.as_ptr().mtu().read().to_ne()
	} else {
		// Otherwise, we can just use the MTU we want to use
		mtu()
	}
}

fn determine_rx_buf_size(dev_cfg: &NetDevCfg) -> u32 {
	// See Virtio specification v1.1 - 5.1.6.3.1 and 5.1.4.2

	// Our desired minimum buffer size - we want it to be at least the MTU generally
	let mut min_buf_size = determine_mtu(dev_cfg).into();

	// If VIRTIO_NET_F_MRG_RXBUF is negotiated, each buffer MUST be at least the size of the struct virtio_net_hdr.
	// We just use MTU in that case, but otherwise...
	if dev_cfg.features.contains(virtio::net::F::MRG_RXBUF)
		&& let Some(my_mrg_rxbuf_size) = hermit_var!("HERMIT_MRG_RXBUF_SIZE")
	{
		let my_mrg_rxbuf_size = u32::from_str(&my_mrg_rxbuf_size).unwrap();
		assert!(
			my_mrg_rxbuf_size > 0,
			"VIRTIO does not allow buffer elements of size 0."
		);
		min_buf_size = my_mrg_rxbuf_size;
	} else {
		// If [...] are negotiated, the driver SHOULD populate the receive queue(s) with buffers of at least 65562 bytes.
		if dev_cfg.features.contains(virtio::net::F::GUEST_TSO4)
			|| dev_cfg.features.contains(virtio::net::F::GUEST_TSO6)
			|| dev_cfg.features.contains(virtio::net::F::GUEST_UFO)
		{
			min_buf_size = u32::max(min_buf_size, 65562 - size_of::<Hdr>() as u32);
		} else {
			// Otherwise, the driver SHOULD populate the receive queue(s) with buffers of at least 1526 bytes.
			min_buf_size = u32::max(min_buf_size, 1526 - size_of::<Hdr>() as u32);
		}
	}

	min_buf_size
}

pub struct RxQueues {
	vqs: Vec<VirtQueue>,
	buf_size: u32,
}

impl RxQueues {
	pub fn new(vqs: Vec<VirtQueue>, dev_cfg: &NetDevCfg) -> Self {
		Self {
			vqs,
			buf_size: determine_rx_buf_size(dev_cfg),
		}
	}

	/// Adds a given queue to the underlying vector and populates the queue with RecvBuffers.
	///
	/// Queues are all populated according to Virtio specification v1.1. - 5.1.6.3.1
	fn add(&mut self, mut vq: VirtQueue) {
		let num_bufs = vq.size() / constants::BUFF_PER_PACKET;
		fill_queue(&mut vq, num_bufs, self.buf_size);
		self.vqs.push(vq);
	}

	fn get_next(&mut self) -> Option<UsedBufferToken> {
		self.vqs[0].try_recv().ok()
	}

	fn enable_notifs(&mut self) {
		for vq in &mut self.vqs {
			vq.enable_notifs();
		}
	}

	fn disable_notifs(&mut self) {
		for vq in &mut self.vqs {
			vq.disable_notifs();
		}
	}

	fn has_packet(&self) -> bool {
		self.vqs.iter().any(|vq| vq.has_used_buffers())
	}
}

fn buffer_token_from_hdr(
	hdr: Box<MaybeUninit<Hdr>, DeviceAlloc>,
	buf_size: u32,
) -> AvailBufferToken {
	AvailBufferToken::new(SmallVec::new(), {
		SmallVec::from_buf([
			BufferElem::Sized(hdr),
			BufferElem::Vector(Vec::with_capacity_in(
				buf_size.try_into().unwrap(),
				DeviceAlloc,
			)),
		])
	})
	.unwrap()
}

fn fill_queue(vq: &mut VirtQueue, num_bufs: u16, buf_size: u32) {
	for _ in 0..num_bufs {
		let buff_tkn = buffer_token_from_hdr(Box::<Hdr, _>::new_uninit_in(DeviceAlloc), buf_size);

		// BufferTokens are directly provided to the queue
		// TransferTokens are directly dispatched
		// Transfers will be awaited at the queue
		if let Err(err) = vq.dispatch(buff_tkn, false, BufferType::Direct) {
			error!("{err:#?}");
			break;
		}
	}
}

/// Structure which handles transmission of packets and delegation
/// to the respective queue structures.
pub struct TxQueues {
	vqs: Vec<VirtQueue>,
	buf_size: u32,
}

impl TxQueues {
	pub fn new(vqs: Vec<VirtQueue>, dev_cfg: &NetDevCfg) -> Self {
		Self {
			vqs,
			buf_size: determine_mtu(dev_cfg).into(),
		}
	}

	#[allow(dead_code)]
	fn enable_notifs(&mut self) {
		for vq in &mut self.vqs {
			vq.enable_notifs();
		}
	}

	#[allow(dead_code)]
	fn disable_notifs(&mut self) {
		for vq in &mut self.vqs {
			vq.disable_notifs();
		}
	}

	/// Polls all queues for buffers whose transmission has been completed and returns the number of such buffers.
	fn poll(&mut self) -> u32 {
		let mut released_buffers = 0u32;
		for vq in &mut self.vqs {
			// We don't do anything with the buffers but we need to receive them for the
			// ring slots to be emptied and the memory from the previous transfers to be freed.
			while vq.try_recv().is_ok() {
				released_buffers += 1;
			}
		}
		released_buffers
	}

	fn add(&mut self, vq: VirtQueue) {
		// Currently we are doing nothing with the additional queues. They are inactive and might be used in the
		// future
		self.vqs.push(vq);
	}
}

pub(crate) struct Uninit;
pub(crate) struct Init {
	pub(super) mtu: u16,
	pub(super) ctrl_vq: Option<VirtQueue>,
	pub(super) recv_vqs: RxQueues,
	pub(super) send_vqs: TxQueues,
	/// Capacity in number of buffer descriptors, not frames.
	pub(super) send_capacity: u32,
}

/// Virtio network driver struct.
///
/// Struct allows to control devices virtqueues as also
/// the device itself.
pub(crate) struct VirtioNetDriver<T = Init> {
	pub(super) dev_cfg: NetDevCfg,
	pub(super) com_cfg: ComCfg,
	pub(super) isr_stat: IsrStatus,
	pub(super) notif_cfg: NotifCfg,

	pub(super) inner: T,

	pub(super) num_vqs: u16,
	pub(super) irq: InterruptLine,
	pub(super) checksums: ChecksumCapabilities,
}

pub struct TxToken<'a> {
	send_vqs: &'a mut TxQueues,
	checksums: ChecksumCapabilities,
	send_capacity: &'a mut u32,
}

impl Drop for TxToken<'_> {
	fn drop(&mut self) {
		*self.send_capacity += u32::from(BUFF_PER_PACKET);
	}
}

impl smoltcp::phy::TxToken for TxToken<'_> {
	fn consume<R, F>(self, len: usize, f: F) -> R
	where
		F: FnOnce(&mut [u8]) -> R,
	{
		// When the token is consumed, the capacity cannot be returned until its buffer is marked by the device as used.
		// Thus, we bypass the Drop implementation that would do that prematurely and let the call to poll in the next
		// call to this function return the capacity.
		let mut token = ManuallyDrop::new(self);
		assert!(len <= usize::try_from(token.send_vqs.buf_size).unwrap());
		let mut packet = Vec::with_capacity_in(len, DeviceAlloc);
		let result = unsafe {
			let result = f(packet.spare_capacity_mut().assume_init_mut());
			packet.set_len(len);
			result
		};

		let mut header = Box::new_in(<Hdr as Default>::default(), DeviceAlloc);

		// If a checksum calculation by the host is necessary, we have to inform the host within the header
		// see Virtio specification 5.1.6.2
		if let Some((ip_header_len, csum_offset)) =
			VirtioNetDriver::should_request_checksum(&token.checksums, &mut packet)
		{
			header.flags = HdrF::NEEDS_CSUM;
			header.csum_start =
				(u16::try_from(ETHERNET_HEADER_LEN).unwrap() + ip_header_len).into();
			header.csum_offset = csum_offset.into();
		}

		let buff_tkn = AvailBufferToken::new(
			SmallVec::from_buf([BufferElem::Sized(header), BufferElem::Vector(packet)]),
			SmallVec::new(),
		)
		.unwrap();

		token.send_vqs.vqs[0]
			.dispatch(buff_tkn, false, BufferType::Direct)
			.unwrap();

		result
	}
}

pub struct RxToken<'a> {
	recv_vqs: &'a mut RxQueues,
	is_mrg_rxbuf_enabled: bool,
	checksums: ChecksumCapabilities,
}

impl RxToken<'_> {
	/// If we advertised receive checksum offload to smoltcp, we need to validate the packet
	/// either by checking its virtio-net headers or checksum. Otherwise, it's smoltcp's responsibility
	/// to validate the frame and we can pass the frame directly.
	fn is_ethernet_frame_passable(&self, hdr: &Hdr, frame: &[u8]) -> bool {
		// Nothing is offloaded to the device. We can pass the frame right off to smoltcp.
		if self.checksums.tcp.rx() && self.checksums.udp.rx() {
			return true;
		}

		let Ok(ethernet_frame) = EthernetFrame::new_checked(frame) else {
			return false;
		};

		// We are receiving a frame that was sent by another virtio-net driver on the same host.
		// Normally, the device should have filled in the checksum but passed the buffers right along
		// instead as checksumming is not necessary for two guests on the same host.
		if hdr.flags.contains(virtio::net::HdrF::NEEDS_CSUM) {
			return true;
		}

		// We cannot benefit from the same host optimization but we've promised smoltcp to only pass frames
		// that are validated so we need to do the validation ourselves.
		match ethernet_frame.ethertype() {
			smoltcp::wire::EthernetProtocol::Ipv4 => {
				let Ok(ip_packet) = Ipv4Packet::new_checked(ethernet_frame.payload()) else {
					return false;
				};

				// DATA_VALID only validates the outermost packet checksum, which is IPv4 in this case. Thus,
				// it does not save us from validating the layer above IP.
				if !hdr.flags.contains(virtio::net::HdrF::DATA_VALID) && !ip_packet.verify_checksum() {
				    return false;
				}

				Self::is_ip_packet_passable(
					ip_packet.next_header(),
					ip_packet.payload(),
					IpAddress::Ipv4(ip_packet.src_addr()),
					IpAddress::Ipv4(ip_packet.dst_addr()),
					&self.checksums,
				)
			}
			smoltcp::wire::EthernetProtocol::Ipv6 => {
				let Ok(ip_packet) = Ipv6Packet::new_checked(ethernet_frame.payload()) else {
					return false;
				};
				// One level of checksum has been validated and IPv6 headers don't have their own checksums,
				// so the validation from the device must have been for the IP protocol.
				hdr.flags.contains(virtio::net::HdrF::DATA_VALID) || Self::is_ip_packet_passable(
					ip_packet.next_header(),
					ip_packet.payload(),
					IpAddress::Ipv6(ip_packet.src_addr()),
					IpAddress::Ipv6(ip_packet.dst_addr()),
					&self.checksums,
				)
			}
			// ARP packets don't have checksums.
			smoltcp::wire::EthernetProtocol::Arp
			// We should have not taken over the validation of any unknown protocol from smoltcp and may let
			// it take care of it.
			| smoltcp::wire::EthernetProtocol::Unknown(_) => {
				true
			}
		}
	}

	fn is_ip_packet_passable(
		next_header: IpProtocol,
		payload: &[u8],
		src_addr: IpAddress,
		dst_addr: IpAddress,
		checksum_capabilities: &ChecksumCapabilities,
	) -> bool {
		match next_header {
			smoltcp::wire::IpProtocol::Tcp => {
				if checksum_capabilities.tcp.rx() {
					return true;
				}
				let Ok(packet) = TcpPacket::new_checked(payload) else {
					return false;
				};
				packet.verify_checksum(&src_addr, &dst_addr)
			}
			smoltcp::wire::IpProtocol::Udp => {
				if checksum_capabilities.udp.rx() {
					return true;
				}
				let Ok(packet) = UdpPacket::new_checked(payload) else {
					return false;
				};
				packet.verify_checksum(&src_addr, &dst_addr)
			}
			_ => true,
		}
	}
}

impl smoltcp::phy::RxToken for RxToken<'_> {
	fn consume<R, F>(self, f: F) -> R
	where
		F: FnOnce(&[u8]) -> R,
	{
		let Some(mut buffer_tkn) = self.recv_vqs.get_next() else {
			// We overpromised a frame. The best we can do is to provide an empty frame to smoltcp and let it handle it as a faulty reception.
			return f(&[]);
		};
		// Safety: any buffers that do not start with a `Hdr` must have been consumed by the previous call
		// to this function.
		let first_header = unsafe {
			buffer_tkn
				.used_recv_buff
				.pop_front_downcast::<Hdr>()
				.unwrap()
		};
		let first_packet = buffer_tkn.used_recv_buff.pop_front_vec().unwrap();

		// According to VIRTIO spec v1.2 sec. 5.1.6.3.2, "num_buffers will always be 1 if VIRTIO_NET_F_MRG_RXBUF is not negotiated."
		// Unfortunately, NVIDIA MLX5 does not comply with this requirement and we have to manually set the value to the correct one.
		let num_buffers = if self.is_mrg_rxbuf_enabled {
			first_header.num_buffers.to_ne()
		} else {
			1
		};

		let mut combined_packets = first_packet;
		for _ in 1..num_buffers {
			let mut buffer_tkn = self.recv_vqs.get_next().unwrap();
			// The descriptor that was meant for the header of another frame was used for a portion of the current frame's contents.
			// Thus, we cannot cast it to a Hdr.
			let (header_descriptor, used_len) = buffer_tkn.used_recv_buff.pop_front_raw().unwrap();
			combined_packets.extend_from_slice(unsafe {
				core::slice::from_raw_parts((&raw const *header_descriptor).cast::<u8>(), used_len)
			});

			let packet = buffer_tkn.used_recv_buff.pop_front_vec().unwrap();
			combined_packets.extend_from_slice(&packet);

			let header = header_descriptor.downcast::<MaybeUninit<Hdr>>().unwrap();

			let tkn = buffer_token_from_hdr(
				// SAFETY: Box<T> -> Box<MaybeUninit<T>> is sound
				header,
				self.recv_vqs.buf_size,
			);
			self.recv_vqs.vqs[0]
				.dispatch(tkn, false, BufferType::Direct)
				.unwrap();
		}

		let res = if self.is_ethernet_frame_passable(&first_header, &combined_packets) {
			f(&combined_packets)
		} else {
			f(&[])
		};

		let first_tkn = buffer_token_from_hdr(
			// SAFETY: Box<T> -> Box<MaybeUninit<T>> is sound
			unsafe {
				transmute::<Box<Hdr, DeviceAlloc>, Box<MaybeUninit<Hdr>, DeviceAlloc>>(first_header)
			},
			self.recv_vqs.buf_size,
		);
		self.recv_vqs.vqs[0]
			.dispatch(first_tkn, false, BufferType::Direct)
			.unwrap();

		res
	}
}

impl NetworkDriver for VirtioNetDriver<Init> {
	/// Returns the mac address of the device.
	/// If VIRTIO_NET_F_MAC is not set, the function panics currently!
	fn get_mac_address(&self) -> [u8; 6] {
		if self.dev_cfg.features.contains(virtio::net::F::MAC) {
			self.com_cfg
				.device_config_space()
				.read_config_with(|| self.dev_cfg.raw.as_ptr().mac().read())
		} else {
			unreachable!("Currently VIRTIO_NET_F_MAC must be negotiated!")
		}
	}

	#[allow(dead_code)]
	fn has_packet(&self) -> bool {
		self.inner.recv_vqs.has_packet()
	}

	fn set_polling_mode(&mut self, value: bool) {
		if value {
			self.disable_interrupts();
		} else {
			self.enable_interrupts();
		}
	}

	fn handle_interrupt(&mut self) {
		let status = self.isr_stat.is_queue_interrupt();

		#[cfg(not(feature = "pci"))]
		if status.contains(virtio::mmio::InterruptStatus::CONFIGURATION_CHANGE_NOTIFICATION) {
			info!("Configuration changes are not possible! Aborting");
			todo!("Implement possibility to change config on the fly...")
		}

		#[cfg(feature = "pci")]
		if status.contains(virtio::pci::IsrStatus::DEVICE_CONFIGURATION_INTERRUPT) {
			info!("Configuration changes are not possible! Aborting");
			todo!("Implement possibility to change config on the fly...")
		}

		self.isr_stat.acknowledge();
	}
}

impl smoltcp::phy::Device for VirtioNetDriver {
	type TxToken<'a> = TxToken<'a>;
	type RxToken<'a> = RxToken<'a>;

	fn capabilities(&self) -> DeviceCapabilities {
		let mut device_capabilities = DeviceCapabilities::default();
		device_capabilities.medium = smoltcp::phy::Medium::Ethernet;
		device_capabilities.max_transmission_unit = self.inner.mtu.into();
		device_capabilities.max_burst_size =
			Some(usize::try_from(self.inner.send_capacity).unwrap() / usize::from(BUFF_PER_PACKET));
		device_capabilities.checksum = self.checksums.clone();
		device_capabilities
	}

	fn receive(
		&mut self,
		_timestamp: smoltcp::time::Instant,
	) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
		if self.inner.recv_vqs.has_packet() && {
			self.free_up_send_capacity();
			self.inner.send_capacity >= u32::from(BUFF_PER_PACKET)
		} {
			self.inner.send_capacity -= u32::from(BUFF_PER_PACKET);
			Some((
				RxToken {
					recv_vqs: &mut self.inner.recv_vqs,
					is_mrg_rxbuf_enabled: self.dev_cfg.features.contains(virtio::net::F::MRG_RXBUF),
					checksums: self.checksums.clone(),
				},
				TxToken {
					send_vqs: &mut self.inner.send_vqs,
					checksums: self.checksums.clone(),
					send_capacity: &mut self.inner.send_capacity,
				},
			))
		} else {
			None
		}
	}

	fn transmit(&mut self, _timestamp: smoltcp::time::Instant) -> Option<Self::TxToken<'_>> {
		self.free_up_send_capacity();
		if self.inner.send_capacity >= u32::from(BUFF_PER_PACKET) {
			self.inner.send_capacity -= u32::from(BUFF_PER_PACKET);
			Some(TxToken {
				send_vqs: &mut self.inner.send_vqs,
				checksums: self.checksums.clone(),
				send_capacity: &mut self.inner.send_capacity,
			})
		} else {
			None
		}
	}
}

impl Driver for VirtioNetDriver<Init> {
	fn get_interrupt_number(&self) -> InterruptLine {
		self.irq
	}

	fn get_name(&self) -> &'static str {
		"virtio"
	}
}

// Backend-independent interface for Virtio network driver
impl VirtioNetDriver<Init> {
	#[cfg(feature = "pci")]
	pub fn get_dev_id(&self) -> u16 {
		self.dev_cfg.dev_id
	}

	/// Returns the current status of the device, if VIRTIO_NET_F_STATUS
	/// has been negotiated. Otherwise assumes an active device.
	#[cfg(not(feature = "pci"))]
	pub fn dev_status(&self) -> virtio::net::S {
		if self.dev_cfg.features.contains(virtio::net::F::STATUS) {
			self.dev_cfg.raw.as_ptr().status().read()
		} else {
			virtio::net::S::LINK_UP
		}
	}

	/// Returns the links status.
	/// If feature VIRTIO_NET_F_STATUS has not been negotiated, then we assume the link is up!
	#[cfg(feature = "pci")]
	pub fn is_link_up(&self) -> bool {
		if self.dev_cfg.features.contains(virtio::net::F::STATUS) {
			self.dev_cfg
				.raw
				.as_ptr()
				.status()
				.read()
				.contains(virtio::net::S::LINK_UP)
		} else {
			true
		}
	}

	#[allow(dead_code)]
	pub fn is_announce(&self) -> bool {
		if self.dev_cfg.features.contains(virtio::net::F::STATUS) {
			self.dev_cfg
				.raw
				.as_ptr()
				.status()
				.read()
				.contains(virtio::net::S::ANNOUNCE)
		} else {
			false
		}
	}

	/// Returns the maximal number of virtqueue pairs allowed. This is the
	/// dominant setting to define the number of virtqueues for the network
	/// device and overrides the num_vq field in the common config.
	///
	/// Returns 1 (i.e. minimum number of pairs) if VIRTIO_NET_F_MQ is not set.
	#[allow(dead_code)]
	pub fn get_max_vq_pairs(&self) -> u16 {
		if self.dev_cfg.features.contains(virtio::net::F::MQ) {
			self.dev_cfg
				.raw
				.as_ptr()
				.max_virtqueue_pairs()
				.read()
				.to_ne()
		} else {
			1
		}
	}

	pub fn disable_interrupts(&mut self) {
		// For send and receive queues?
		// Only for receive? Because send is off anyway?
		self.inner.recv_vqs.disable_notifs();
	}

	pub fn enable_interrupts(&mut self) {
		// For send and receive queues?
		// Only for receive? Because send is off anyway?
		self.inner.recv_vqs.enable_notifs();
	}

	/// If necessary, sets the TCP or UDP checksum field to the checksum of the
	/// pseudo-header and returns the IP header length and the checksum offset.
	/// Otherwise, returns None.
	fn should_request_checksum<T: AsRef<[u8]> + AsMut<[u8]>>(
		checksums: &ChecksumCapabilities,
		frame: T,
	) -> Option<(u16, u16)> {
		if checksums.tcp.tx() && checksums.udp.tx() {
			return None;
		}

		let ip_header_len: u16;
		let ip_packet_len: usize;
		let protocol;
		let pseudo_header_checksum;
		let mut ethernet_frame = EthernetFrame::new_unchecked(frame);
		match ethernet_frame.ethertype() {
			smoltcp::wire::EthernetProtocol::Ipv4 => {
				let ip_packet = Ipv4Packet::new_unchecked(&*ethernet_frame.payload_mut());
				ip_header_len = ip_packet.header_len().into();
				ip_packet_len = ip_packet.total_len().into();
				protocol = ip_packet.next_header();
				pseudo_header_checksum =
					partial_checksum::ipv4_pseudo_header_partial_checksum(&ip_packet);
			}
			smoltcp::wire::EthernetProtocol::Ipv6 => {
				let ip_packet = Ipv6Packet::new_unchecked(&*ethernet_frame.payload_mut());
				ip_header_len = ip_packet.header_len().try_into().expect(
					"VIRTIO does not support IP headers that are longer than u16::MAX bytes.",
				);
				ip_packet_len = ip_packet.total_len();
				protocol = ip_packet.next_header();
				pseudo_header_checksum =
					partial_checksum::ipv6_pseudo_header_partial_checksum(&ip_packet);
			}
			// If the Ethernet protocol is not one of these two above, for which we know there may be a checksum field,
			// we default to not asking for checksum, as otherwise the frame will be corrupted by the device trying
			// to write the checksum.
			_ => return None,
		};

		let csum_offset;
		let ip_payload = &mut ethernet_frame.payload_mut()[ip_header_len.into()..ip_packet_len];
		// Like the Ethernet protocol check, we check for IP protocols for which we know the location of the checksum field.
		if protocol == smoltcp::wire::IpProtocol::Tcp && !checksums.tcp.tx() {
			let mut tcp_packet = smoltcp::wire::TcpPacket::new_unchecked(ip_payload);
			tcp_packet.set_checksum(pseudo_header_checksum);
			csum_offset = 16;
		} else if protocol == smoltcp::wire::IpProtocol::Udp && !checksums.udp.tx() {
			let mut udp_packet = smoltcp::wire::UdpPacket::new_unchecked(ip_payload);
			udp_packet.set_checksum(pseudo_header_checksum);
			csum_offset = 6;
		} else {
			return None;
		};

		Some((ip_header_len, csum_offset))
	}

	fn free_up_send_capacity(&mut self) {
		// We need to poll to get the queue to remove elements from the table and open up capacity if possible.
		self.inner.send_capacity += self.inner.send_vqs.poll() * u32::from(BUFF_PER_PACKET);
	}
}

impl VirtioNetDriver<Uninit> {
	/// Initializes the device in adherence to specification. Returns Some(VirtioNetError)
	/// upon failure and None in case everything worked as expected.
	///
	/// See Virtio specification v1.1. - 3.1.1.
	///                      and v1.1. - 5.1.5
	pub fn init_dev(mut self) -> Result<VirtioNetDriver<Init>, VirtioNetError> {
		// Reset
		self.com_cfg.reset_dev();

		// Indicate device, that OS noticed it
		self.com_cfg.ack_dev();

		// Indicate device, that driver is able to handle it
		self.com_cfg.set_drv();

		let minimal_features = virtio::net::F::VERSION_1 | virtio::net::F::MAC;

		// If wanted, push new features into feats here:
		let features = minimal_features
			// Indirect descriptors can be used
			| virtio::net::F::INDIRECT_DESC
			// Packed Vq can be used
			| virtio::net::F::RING_PACKED
			| virtio::net::F::NOTIFICATION_DATA
			// MTU setting can be used
			| virtio::net::F::MTU
			// Driver can merge receive buffers
			| virtio::net::F::MRG_RXBUF
			// the link status can be announced
			| virtio::net::F::STATUS
			// control queue support
			| virtio::net::F::CTRL_VQ
			// Multiqueue support
			| virtio::net::F::MQ
			// Checksum calculation can partially be offloaded to the device
			| virtio::net::F::CSUM
			// Partially checksummed frames can be received
			| virtio::net::F::GUEST_CSUM;

		// Currently the driver does NOT support the features below.
		// In order to provide functionality for these, the driver
		// needs to take care of calculating checksum.
		// | virtio::net::F::GUEST_TSO4
		// | virtio::net::F::GUEST_TSO6

		let negotiated_features = self
			.com_cfg
			.control_registers()
			.negotiate_features(features);

		if !negotiated_features.contains(minimal_features) {
			error!("Device features set, does not satisfy minimal features needed. Aborting!");
			return Err(VirtioNetError::FailFeatureNeg(self.dev_cfg.dev_id));
		}

		// Indicates the device, that the current feature set is final for the driver
		// and will not be changed.
		self.com_cfg.features_ok();

		// Checks if the device has accepted final set. This finishes feature negotiation.
		if self.com_cfg.check_features() {
			info!(
				"Features have been negotiated between virtio network device {:x} and driver.",
				self.dev_cfg.dev_id
			);
			// Set feature set in device config fur future use.
			self.dev_cfg.features = negotiated_features;
		} else {
			error!("The device does not support our subset of features.");
			return Err(VirtioNetError::FailFeatureNeg(self.dev_cfg.dev_id));
		}

		let mut inner = Init {
			mtu: determine_mtu(&self.dev_cfg),
			ctrl_vq: None,
			recv_vqs: RxQueues::new(Vec::new(), &self.dev_cfg),
			send_vqs: TxQueues::new(Vec::new(), &self.dev_cfg),
			send_capacity: 0,
		};

		debug!("Using RX buffer size of {}", inner.recv_vqs.buf_size);

		self.dev_spec_init(&mut inner)?;
		info!(
			"Device specific initialization for Virtio network device {:x} finished",
			self.dev_cfg.dev_id
		);

		// At this point the device is "live"
		self.com_cfg.drv_ok();

		if self.dev_cfg.features.contains(virtio::net::F::CSUM)
			&& self.dev_cfg.features.contains(virtio::net::F::GUEST_CSUM)
		{
			self.checksums.udp = Checksum::None;
			self.checksums.tcp = Checksum::None;
		} else if self.dev_cfg.features.contains(virtio::net::F::CSUM) {
			self.checksums.udp = Checksum::Rx;
			self.checksums.tcp = Checksum::Rx;
		} else if self.dev_cfg.features.contains(virtio::net::F::GUEST_CSUM) {
			self.checksums.udp = Checksum::Tx;
			self.checksums.tcp = Checksum::Tx;
		}
		debug!("{:?}", self.checksums);

		Ok(VirtioNetDriver {
			dev_cfg: self.dev_cfg,
			com_cfg: self.com_cfg,
			isr_stat: self.isr_stat,
			notif_cfg: self.notif_cfg,
			inner,
			num_vqs: self.num_vqs,
			irq: self.irq,
			checksums: self.checksums,
		})
	}

	/// Device Specific initialization according to Virtio specifictation v1.1. - 5.1.5
	fn dev_spec_init(&mut self, inner: &mut Init) -> Result<(), VirtioNetError> {
		self.virtqueue_init(inner)?;
		info!("Network driver successfully initialized virtqueues.");

		// Add a control if feature is negotiated
		if self.dev_cfg.features.contains(virtio::net::F::CTRL_VQ) {
			let mut ctrl_vq = if self.dev_cfg.features.contains(virtio::net::F::RING_PACKED) {
				VirtQueue::Packed(
					PackedVq::new(
						&mut self.com_cfg,
						&self.notif_cfg,
						VIRTIO_MAX_QUEUE_SIZE,
						self.num_vqs,
						self.dev_cfg.features.into(),
					)
					.unwrap(),
				)
			} else {
				VirtQueue::Split(
					SplitVq::new(
						&mut self.com_cfg,
						&self.notif_cfg,
						VIRTIO_MAX_QUEUE_SIZE,
						self.num_vqs,
						self.dev_cfg.features.into(),
					)
					.unwrap(),
				)
			};

			ctrl_vq.enable_notifs();
			inner.ctrl_vq = Some(ctrl_vq);
		}

		Ok(())
	}

	/// Initialize virtqueues via the queue interface and populates receiving queues
	fn virtqueue_init(&mut self, inner: &mut Init) -> Result<(), VirtioNetError> {
		// We are assuming here, that the device single source of truth is the
		// device specific configuration. Hence we do NOT check if
		//
		// max_virtqueue_pairs + 1 < num_queues
		//
		// - the plus 1 is due to the possibility of an existing control queue
		// - the num_queues is found in the ComCfg struct of the device and defines the maximal number
		// of supported queues.
		if self.dev_cfg.features.contains(virtio::net::F::MQ) {
			if self
				.dev_cfg
				.raw
				.as_ptr()
				.max_virtqueue_pairs()
				.read()
				.to_ne() * 2 >= MAX_NUM_VQ
			{
				self.num_vqs = MAX_NUM_VQ;
			} else {
				self.num_vqs = self
					.dev_cfg
					.raw
					.as_ptr()
					.max_virtqueue_pairs()
					.read()
					.to_ne() * 2;
			}
		} else {
			// Minimal number of virtqueues defined in the standard v1.1. - 5.1.5 Step 1
			self.num_vqs = 2;
		}

		// The loop is running from 0 to num_vqs and the indexes are provided in this way
		// in order to allow the indexes of the queues to be in a form of:
		//
		// index i for receive queue
		// index i+1 for send queue
		//
		// as it is wanted by the network network device.
		// see Virtio specification v1.1. - 5.1.2
		// Assure that we have always an even number of queues (i.e. pairs of queues).
		assert_eq!(self.num_vqs % 2, 0);

		for i in 0..(self.num_vqs / 2) {
			if self.dev_cfg.features.contains(virtio::net::F::RING_PACKED) {
				let mut vq = PackedVq::new(
					&mut self.com_cfg,
					&self.notif_cfg,
					VIRTIO_MAX_QUEUE_SIZE,
					2 * i,
					self.dev_cfg.features.into(),
				)
				.unwrap();
				// Interrupt for receiving packets is wanted
				vq.enable_notifs();

				inner.recv_vqs.add(VirtQueue::Packed(vq));

				let mut vq = PackedVq::new(
					&mut self.com_cfg,
					&self.notif_cfg,
					VIRTIO_MAX_QUEUE_SIZE,
					2 * i + 1,
					self.dev_cfg.features.into(),
				)
				.unwrap();
				// Interrupt for communicating that a sent packet left, is not needed
				vq.disable_notifs();

				inner.send_capacity += u32::from(vq.size());
				inner.send_vqs.add(VirtQueue::Packed(vq));
			} else {
				let mut vq = SplitVq::new(
					&mut self.com_cfg,
					&self.notif_cfg,
					VIRTIO_MAX_QUEUE_SIZE,
					2 * i,
					self.dev_cfg.features.into(),
				)
				.unwrap();
				// Interrupt for receiving packets is wanted
				vq.enable_notifs();

				inner.recv_vqs.add(VirtQueue::Split(vq));

				let mut vq = SplitVq::new(
					&mut self.com_cfg,
					&self.notif_cfg,
					VIRTIO_MAX_QUEUE_SIZE,
					2 * i + 1,
					self.dev_cfg.features.into(),
				)
				.unwrap();
				// Interrupt for communicating that a sent packet left, is not needed
				vq.disable_notifs();
				inner.send_capacity += u32::from(vq.size());
				inner.send_vqs.add(VirtQueue::Split(vq));
			}
		}

		Ok(())
	}
}

pub mod constants {
	// Configuration constants
	pub const MAX_NUM_VQ: u16 = 2;
	pub(super) const BUFF_PER_PACKET: u16 = 2;
}

/// Error module of virtios network driver. Containing the (VirtioNetError)[VirtioNetError]
/// enum.
pub mod error {
	use thiserror::Error;

	/// Network drivers error enum.
	#[derive(Error, Debug, Copy, Clone)]
	pub enum VirtioNetError {
		#[cfg(feature = "pci")]
		#[error(
			"Virtio network driver failed, for device {0:x}, due to a missing or malformed device config!"
		)]
		NoDevCfg(u16),

		#[error(
			"Virtio network driver failed, for device {0:x}, device did not acknowledge negotiated feature set!"
		)]
		FailFeatureNeg(u16),
	}
}

/// The checksum functions in this module only calculate the one's complement sum for the pseudo-header
/// and their results are meant to be combined with the TCP payload to calculate the real checksum.
/// They are only useful for the VIRTIO driver with the checksum offloading feature.
///
/// The calculations here can theoretically be made faster by exploiting the properties described in
/// [RFC 1071 section 2](https://www.rfc-editor.org/rfc/rfc1071).
mod partial_checksum {
	use smoltcp::wire::{Ipv4Packet, Ipv6Packet};

	fn addr_sum<const N: usize>(addr: &[u8; N]) -> u16 {
		let mut sum = 0;
		const CHUNK_SIZE: usize = size_of::<u16>();
		for i in 0..(N / CHUNK_SIZE) {
			sum = ones_complement_add(
				sum,
				(u16::from(addr[CHUNK_SIZE * i]) << 8) | u16::from(addr[CHUNK_SIZE * i + 1]),
			);
		}
		sum
	}

	/// Calculates the checksum for the IPv4 pseudo-header as described in
	/// [RFC 9293 subsection 3.1](https://www.rfc-editor.org/rfc/rfc9293.html#section-3.1-6.18.1) WITHOUT the final inversion.
	pub(super) fn ipv4_pseudo_header_partial_checksum<T: AsRef<[u8]>>(
		packet: &Ipv4Packet<T>,
	) -> u16 {
		let padded_protocol = u16::from(u8::from(packet.next_header()));
		let payload_len = packet.total_len() - u16::from(packet.header_len());

		let mut sum = addr_sum(&packet.src_addr().octets());
		sum = ones_complement_add(sum, addr_sum(&packet.dst_addr().octets()));
		sum = ones_complement_add(sum, padded_protocol);
		ones_complement_add(sum, payload_len)
	}

	/// Calculates the checksum for the IPv6 pseudo-header as described in
	/// [RFC 8200 subsection 8.1](https://www.rfc-editor.org/rfc/rfc8200.html#section-8.1) WITHOUT the final inversion.
	pub(super) fn ipv6_pseudo_header_partial_checksum<T: AsRef<[u8]>>(
		packet: &Ipv6Packet<T>,
	) -> u16 {
		warn!("The IPv6 partial checksum implementation is untested!");
		let padded_protocol = u16::from(u8::from(packet.next_header()));

		let mut sum = addr_sum(&packet.src_addr().octets());
		sum = ones_complement_add(sum, addr_sum(&packet.dst_addr().octets()));
		sum = ones_complement_add(sum, packet.payload_len());
		ones_complement_add(sum, padded_protocol)
	}

	/// Implements one's complement checksum as described in [RFC 1071 section 1](https://www.rfc-editor.org/rfc/rfc1071#section-1).
	fn ones_complement_add(lhs: u16, rhs: u16) -> u16 {
		let (sum, overflow) = u16::overflowing_add(lhs, rhs);
		sum + u16::from(overflow)
	}
}
