//! A module containing a virtio network driver.
//!
//! The module contains ...

cfg_if::cfg_if! {
	if #[cfg(feature = "pci")] {
		mod pci;
	} else {
		mod mmio;
	}
}

use alloc::boxed::Box;
use alloc::vec::Vec;

use smoltcp::phy::{Checksum, ChecksumCapabilities};
use smoltcp::wire::{ETHERNET_HEADER_LEN, EthernetFrame, Ipv4Packet, Ipv6Packet};
use virtio::net::{ConfigVolatileFieldAccess, Hdr, HdrF};
use virtio::{DeviceConfigSpace, FeatureBits};
use volatile::VolatileRef;
use volatile::access::ReadOnly;

use self::constants::MAX_NUM_VQ;
use self::error::VirtioNetError;
use crate::config::VIRTIO_MAX_QUEUE_SIZE;
use crate::drivers::net::NetworkDriver;
#[cfg(not(feature = "pci"))]
use crate::drivers::virtio::transport::mmio::{ComCfg, IsrStatus, NotifCfg};
#[cfg(feature = "pci")]
use crate::drivers::virtio::transport::pci::{ComCfg, IsrStatus, NotifCfg};
use crate::drivers::virtio::virtqueue::packed::PackedVq;
use crate::drivers::virtio::virtqueue::split::SplitVq;
use crate::drivers::virtio::virtqueue::{
	AvailBufferToken, BufferElem, BufferType, UsedBufferToken, Virtq, VqIndex, VqSize,
};
use crate::drivers::{Driver, InterruptLine};
use crate::executor::device::{RxToken, TxToken};
use crate::mm::device_alloc::DeviceAlloc;

/// A wrapper struct for the raw configuration structure.
/// Handling the right access to fields, as some are read-only
/// for the driver.
pub(crate) struct NetDevCfg {
	pub raw: VolatileRef<'static, virtio::net::Config, ReadOnly>,
	pub dev_id: u16,
	pub features: virtio::net::F,
}

pub struct CtrlQueue(Option<Box<dyn Virtq>>);

impl CtrlQueue {
	pub fn new(vq: Option<Box<dyn Virtq>>) -> Self {
		CtrlQueue(vq)
	}
}

pub struct RxQueues {
	vqs: Vec<Box<dyn Virtq>>,
	packet_size: u32,
}

impl RxQueues {
	pub fn new(vqs: Vec<Box<dyn Virtq>>, dev_cfg: &NetDevCfg) -> Self {
		// See Virtio specification v1.1 - 5.1.6.3.1
		//
		let packet_size = if dev_cfg.features.contains(virtio::net::F::MRG_RXBUF) {
			1514
		} else {
			dev_cfg.raw.as_ptr().mtu().read().to_ne().into()
		};

		Self { vqs, packet_size }
	}

	/// Takes care of handling packets correctly which need some processing after being received.
	/// This currently include nothing. But in the future it might include among others:
	/// * Calculating missing checksums
	/// * Merging receive buffers, by simply checking the poll_queue (if VIRTIO_NET_F_MRG_BUF)
	fn post_processing(_buffer_tkn: &mut UsedBufferToken) -> Result<(), VirtioNetError> {
		Ok(())
	}

	/// Adds a given queue to the underlying vector and populates the queue with RecvBuffers.
	///
	/// Queues are all populated according to Virtio specification v1.1. - 5.1.6.3.1
	fn add(&mut self, mut vq: Box<dyn Virtq>) {
		const BUFF_PER_PACKET: u16 = 2;
		let num_packets: u16 = u16::from(vq.size()) / BUFF_PER_PACKET;
		fill_queue(vq.as_mut(), num_packets, self.packet_size);
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

fn fill_queue(vq: &mut dyn Virtq, num_packets: u16, packet_size: u32) {
	for _ in 0..num_packets {
		let buff_tkn = match AvailBufferToken::new(
			vec![],
			vec![
				BufferElem::Sized(Box::<Hdr, _>::new_uninit_in(DeviceAlloc)),
				BufferElem::Vector(Vec::with_capacity_in(
					packet_size.try_into().unwrap(),
					DeviceAlloc,
				)),
			],
		) {
			Ok(tkn) => tkn,
			Err(_vq_err) => {
				error!("Setup of network queue failed, which should not happen!");
				panic!("setup of network queue failed!");
			}
		};

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
	vqs: Vec<Box<dyn Virtq>>,
	/// Indicates, whether the Driver/Device are using multiple
	/// queues for communication.
	packet_length: u32,
}

impl TxQueues {
	pub fn new(vqs: Vec<Box<dyn Virtq>>, dev_cfg: &NetDevCfg) -> Self {
		let packet_length = if dev_cfg.features.contains(virtio::net::F::GUEST_TSO4)
			| dev_cfg.features.contains(virtio::net::F::GUEST_TSO6)
			| dev_cfg.features.contains(virtio::net::F::GUEST_UFO)
		{
			0x0001_000e
		} else {
			dev_cfg.raw.as_ptr().mtu().read().to_ne().into()
		};

		Self { vqs, packet_length }
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

	fn poll(&mut self) {
		for vq in &mut self.vqs {
			// We don't do anything with the buffers but we need to receive them for the
			// ring slots to be emptied and the memory from the previous transfers to be freed.
			while vq.try_recv().is_ok() {}
		}
	}

	fn add(&mut self, vq: Box<dyn Virtq>) {
		// Currently we are doing nothing with the additional queues. They are inactive and might be used in the
		// future
		self.vqs.push(vq);
	}
}

/// Virtio network driver struct.
///
/// Struct allows to control devices virtqueues as also
/// the device itself.
pub(crate) struct VirtioNetDriver {
	pub(super) dev_cfg: NetDevCfg,
	pub(super) com_cfg: ComCfg,
	pub(super) isr_stat: IsrStatus,
	pub(super) notif_cfg: NotifCfg,

	pub(super) ctrl_vq: CtrlQueue,
	pub(super) recv_vqs: RxQueues,
	pub(super) send_vqs: TxQueues,

	pub(super) num_vqs: u16,
	pub(super) mtu: u16,
	pub(super) irq: InterruptLine,
	pub(super) checksums: ChecksumCapabilities,
}

impl NetworkDriver for VirtioNetDriver {
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

	/// Returns the current MTU of the device.
	fn get_mtu(&self) -> u16 {
		self.mtu
	}

	fn get_checksums(&self) -> ChecksumCapabilities {
		self.checksums.clone()
	}

	#[allow(dead_code)]
	fn has_packet(&self) -> bool {
		self.recv_vqs.has_packet()
	}

	/// Provides smoltcp a slice to copy the IP packet and transfer the packet
	/// to the send queue.
	fn send_packet<R, F>(&mut self, len: usize, f: F) -> R
	where
		F: FnOnce(&mut [u8]) -> R,
	{
		// We need to poll to get the queue to remove elements from the table and make space for
		// what we are about to add
		self.send_vqs.poll();

		assert!(len < usize::try_from(self.send_vqs.packet_length).unwrap());
		let mut packet = Vec::with_capacity_in(len, DeviceAlloc);
		let result = unsafe {
			let result = f(packet.spare_capacity_mut().assume_init_mut());
			packet.set_len(len);
			result
		};

		let mut header = Box::new_in(<Hdr as Default>::default(), DeviceAlloc);
		// If a checksum isn't necessary, we have inform the host within the header
		// see Virtio specification 5.1.6.2
		if !self.checksums.tcp.tx() || !self.checksums.udp.tx() {
			header.flags = HdrF::NEEDS_CSUM;
			let ethernet_frame: smoltcp::wire::EthernetFrame<&[u8]> =
				EthernetFrame::new_unchecked(&packet);
			let packet_header_len: u16;
			let protocol;
			match ethernet_frame.ethertype() {
				smoltcp::wire::EthernetProtocol::Ipv4 => {
					let packet = Ipv4Packet::new_unchecked(ethernet_frame.payload());
					packet_header_len = packet.header_len().into();
					protocol = Some(packet.next_header());
				}
				smoltcp::wire::EthernetProtocol::Ipv6 => {
					let packet = Ipv6Packet::new_unchecked(ethernet_frame.payload());
					packet_header_len = packet.header_len().try_into().unwrap();
					protocol = Some(packet.next_header());
				}
				_ => {
					packet_header_len = 0;
					protocol = None;
				}
			}
			header.csum_start =
				(u16::try_from(ETHERNET_HEADER_LEN).unwrap() + packet_header_len).into();
			header.csum_offset = match protocol {
				Some(smoltcp::wire::IpProtocol::Tcp) => 16,
				Some(smoltcp::wire::IpProtocol::Udp) => 6,
				_ => 0,
			}
			.into();
		}

		let buff_tkn = AvailBufferToken::new(
			vec![BufferElem::Sized(header), BufferElem::Vector(packet)],
			vec![],
		)
		.unwrap();

		self.send_vqs.vqs[0]
			.dispatch(buff_tkn, false, BufferType::Direct)
			.unwrap();

		result
	}

	fn receive_packet(&mut self) -> Option<(RxToken, TxToken)> {
		let mut buffer_tkn = self.recv_vqs.get_next()?;
		RxQueues::post_processing(&mut buffer_tkn)
			.inspect_err(|vnet_err| warn!("Post processing failed. Err: {vnet_err:?}"))
			.ok()?;
		let first_header = buffer_tkn.used_recv_buff.pop_front_downcast::<Hdr>()?;
		let first_packet = buffer_tkn.used_recv_buff.pop_front_vec()?;
		trace!("Header: {first_header:?}");

		// According to VIRTIO spec v1.2 sec. 5.1.6.3.2, "num_buffers will always be 1 if VIRTIO_NET_F_MRG_RXBUF is not negotiated."
		// Unfortunately, NVIDIA MLX5 does not comply with this requirement and we have to manually set the value to the correct one.
		let num_buffers = if self.dev_cfg.features.contains(virtio::net::F::MRG_RXBUF) {
			first_header.num_buffers.to_ne()
		} else {
			1
		};

		let mut packets = Vec::with_capacity(num_buffers.into());
		packets.push(first_packet);

		for _ in 1..num_buffers {
			let mut buffer_tkn = self.recv_vqs.get_next().unwrap();
			RxQueues::post_processing(&mut buffer_tkn)
				.inspect_err(|vnet_err| warn!("Post processing failed. Err: {vnet_err:?}"))
				.ok()?;
			let _header = buffer_tkn.used_recv_buff.pop_front_downcast::<Hdr>()?;
			let packet = buffer_tkn.used_recv_buff.pop_front_vec()?;
			packets.push(packet);
		}

		fill_queue(
			self.recv_vqs.vqs[0].as_mut(),
			num_buffers,
			self.recv_vqs.packet_size,
		);

		let vec_data = packets.into_iter().flatten().collect();

		Some((RxToken::new(vec_data), TxToken::new()))
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

impl Driver for VirtioNetDriver {
	fn get_interrupt_number(&self) -> InterruptLine {
		self.irq
	}

	fn get_name(&self) -> &'static str {
		"virtio"
	}
}

// Backend-independent interface for Virtio network driver
impl VirtioNetDriver {
	#[cfg(feature = "pci")]
	pub fn get_dev_id(&self) -> u16 {
		self.dev_cfg.dev_id
	}

	#[cfg(feature = "pci")]
	pub fn set_failed(&mut self) {
		self.com_cfg.set_failed();
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
		self.recv_vqs.disable_notifs();
	}

	pub fn enable_interrupts(&mut self) {
		// For send and receive queues?
		// Only for receive? Because send is off anyway?
		self.recv_vqs.enable_notifs();
	}

	/// Initializes the device in adherence to specification. Returns Some(VirtioNetError)
	/// upon failure and None in case everything worked as expected.
	///
	/// See Virtio specification v1.1. - 3.1.1.
	///                      and v1.1. - 5.1.5
	pub fn init_dev(&mut self) -> Result<(), VirtioNetError> {
		// Reset
		self.com_cfg.reset_dev();

		// Indicate device, that OS noticed it
		self.com_cfg.ack_dev();

		// Indicate device, that driver is able to handle it
		self.com_cfg.set_drv();

		let minimal_features = virtio::net::F::VERSION_1 | virtio::net::F::MAC;

		// If wanted, push new features into feats here:
		let mut features = minimal_features
			// Indirect descriptors can be used
			| virtio::net::F::INDIRECT_DESC
			// Packed Vq can be used
			| virtio::net::F::RING_PACKED
			| virtio::net::F::NOTIFICATION_DATA
			// Host should avoid the creation of checksums
			| virtio::net::F::CSUM
			// Guest avoids the creation of checksums
			| virtio::net::F::GUEST_CSUM
			// MTU setting can be used
			| virtio::net::F::MTU
			// Driver can merge receive buffers
			| virtio::net::F::MRG_RXBUF
			// the link status can be announced
			| virtio::net::F::STATUS
			// Multiqueue support
			| virtio::net::F::MQ;

		// Currently the driver does NOT support the features below.
		// In order to provide functionality for these, the driver
		// needs to take care of calculating checksum in
		// RxQueues.post_processing()
		// | virtio::net::F::GUEST_TSO4
		// | virtio::net::F::GUEST_TSO6

		// Negotiate features with device. Automatically reduces selected feats in order to meet device capabilities.
		// Aborts in case incompatible features are selected by the driver or the device does not support min_feat_set.
		match self.negotiate_features(features) {
			Ok(()) => info!(
				"Driver found a subset of features for virtio device {:x}. Features are: {features:?}",
				self.dev_cfg.dev_id
			),
			Err(vnet_err) => {
				match vnet_err {
					VirtioNetError::FeatureRequirementsNotMet(features) => {
						error!(
							"Network drivers feature set {features:?} does not satisfy rules in section 5.1.3.1 of specification v1.1. Aborting!"
						);
						return Err(vnet_err);
					}
					VirtioNetError::IncompatibleFeatureSets(drv_feats, dev_feats) => {
						// Create a new matching feature set for device and driver if the minimal set is met!
						if !dev_feats.contains(minimal_features) {
							error!(
								"Device features set, does not satisfy minimal features needed. Aborting!"
							);
							return Err(VirtioNetError::FailFeatureNeg(self.dev_cfg.dev_id));
						}

						let common_features = drv_feats & dev_feats;
						if common_features.is_empty() {
							error!(
								"Feature negotiation failed with minimal feature set. Aborting!"
							);
							return Err(VirtioNetError::FailFeatureNeg(self.dev_cfg.dev_id));
						}
						features = common_features;

						match self.negotiate_features(features) {
							Ok(()) => info!(
								"Driver found a subset of features for virtio device {:x}. Features are: {features:?}",
								self.dev_cfg.dev_id
							),
							Err(vnet_err) => match vnet_err {
								VirtioNetError::FeatureRequirementsNotMet(features) => {
									error!(
										"Network device offers a feature set {features:?} when used completely does not satisfy rules in section 5.1.3.1 of specification v1.1. Aborting!"
									);
									return Err(vnet_err);
								}
								_ => {
									error!(
										"Feature Set after reduction still not usable. Set: {features:?}. Aborting!"
									);
									return Err(vnet_err);
								}
							},
						}
					}
					VirtioNetError::FailFeatureNeg(_) => {
						error!(
							"Wanted set of features is NOT supported by device. Set: {features:?}"
						);
						return Err(vnet_err);
					}
					#[cfg(feature = "pci")]
					VirtioNetError::NoDevCfg(_) => {
						error!("No device config found.");
						return Err(vnet_err);
					}
				}
			}
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
			self.dev_cfg.features = features;
		} else {
			return Err(VirtioNetError::FailFeatureNeg(self.dev_cfg.dev_id));
		}

		self.dev_spec_init()?;
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

		if self.dev_cfg.features.contains(virtio::net::F::MTU) {
			self.mtu = self.dev_cfg.raw.as_ptr().mtu().read().to_ne();
		}

		Ok(())
	}

	/// Negotiates a subset of features, understood and wanted by both the OS
	/// and the device.
	fn negotiate_features(
		&mut self,
		driver_features: virtio::net::F,
	) -> Result<(), VirtioNetError> {
		let device_features = virtio::net::F::from(self.com_cfg.dev_features());

		if device_features.requirements_satisfied() {
			info!("Feature set wanted by network driver are in conformance with specification.");
		} else {
			return Err(VirtioNetError::FeatureRequirementsNotMet(device_features));
		}

		if device_features.contains(driver_features) {
			// If device supports subset of features write feature set to common config
			self.com_cfg.set_drv_features(driver_features.into());
			Ok(())
		} else {
			Err(VirtioNetError::IncompatibleFeatureSets(
				driver_features,
				device_features,
			))
		}
	}

	/// Device Specific initialization according to Virtio specifictation v1.1. - 5.1.5
	fn dev_spec_init(&mut self) -> Result<(), VirtioNetError> {
		self.virtqueue_init()?;
		info!("Network driver successfully initialized virtqueues.");

		// Add a control if feature is negotiated
		if self.dev_cfg.features.contains(virtio::net::F::CTRL_VQ) {
			if self.dev_cfg.features.contains(virtio::net::F::RING_PACKED) {
				self.ctrl_vq = CtrlQueue(Some(Box::new(
					PackedVq::new(
						&mut self.com_cfg,
						&self.notif_cfg,
						VqSize::from(VIRTIO_MAX_QUEUE_SIZE),
						VqIndex::from(self.num_vqs),
						self.dev_cfg.features.into(),
					)
					.unwrap(),
				)));
			} else {
				self.ctrl_vq = CtrlQueue(Some(Box::new(
					SplitVq::new(
						&mut self.com_cfg,
						&self.notif_cfg,
						VqSize::from(VIRTIO_MAX_QUEUE_SIZE),
						VqIndex::from(self.num_vqs),
						self.dev_cfg.features.into(),
					)
					.unwrap(),
				)));
			}

			self.ctrl_vq.0.as_mut().unwrap().enable_notifs();
		}

		Ok(())
	}

	/// Initialize virtqueues via the queue interface and populates receiving queues
	fn virtqueue_init(&mut self) -> Result<(), VirtioNetError> {
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

		// The loop is running from 0 to num_vqs and the indexes are provided to the VqIndex::from function in this way
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
					VqSize::from(VIRTIO_MAX_QUEUE_SIZE),
					VqIndex::from(2 * i),
					self.dev_cfg.features.into(),
				)
				.unwrap();
				// Interrupt for receiving packets is wanted
				vq.enable_notifs();

				self.recv_vqs.add(Box::from(vq));

				let mut vq = PackedVq::new(
					&mut self.com_cfg,
					&self.notif_cfg,
					VqSize::from(VIRTIO_MAX_QUEUE_SIZE),
					VqIndex::from(2 * i + 1),
					self.dev_cfg.features.into(),
				)
				.unwrap();
				// Interrupt for communicating that a sended packet left, is not needed
				vq.disable_notifs();

				self.send_vqs.add(Box::from(vq));
			} else {
				let mut vq = SplitVq::new(
					&mut self.com_cfg,
					&self.notif_cfg,
					VqSize::from(VIRTIO_MAX_QUEUE_SIZE),
					VqIndex::from(2 * i),
					self.dev_cfg.features.into(),
				)
				.unwrap();
				// Interrupt for receiving packets is wanted
				vq.enable_notifs();

				self.recv_vqs.add(Box::from(vq));

				let mut vq = SplitVq::new(
					&mut self.com_cfg,
					&self.notif_cfg,
					VqSize::from(VIRTIO_MAX_QUEUE_SIZE),
					VqIndex::from(2 * i + 1),
					self.dev_cfg.features.into(),
				)
				.unwrap();
				// Interrupt for communicating that a sended packet left, is not needed
				vq.disable_notifs();

				self.send_vqs.add(Box::from(vq));
			}
		}

		Ok(())
	}
}

pub mod constants {
	// Configuration constants
	pub const MAX_NUM_VQ: u16 = 2;
}

/// Error module of virtios network driver. Containing the (VirtioNetError)[VirtioNetError]
/// enum.
pub mod error {
	/// Network drivers error enum.
	#[derive(Debug, Copy, Clone)]
	pub enum VirtioNetError {
		#[cfg(feature = "pci")]
		NoDevCfg(u16),
		FailFeatureNeg(u16),
		/// Set of features does not adhere to the requirements of features
		/// indicated by the specification
		FeatureRequirementsNotMet(virtio::net::F),
		/// The first field contains the feature bits wanted by the driver.
		/// but which are incompatible with the device feature set, second field.
		IncompatibleFeatureSets(virtio::net::F, virtio::net::F),
	}
}
