// Copyright (c) 2020 Frederik Schulz, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

//! A module containing a virtio network driver.
//! 
//! The module contains ...
use arch::x86_64::kernel::pci::PciAdapter;
use arch::x86_64::kernel::pci::error::PciError;
use config::VIRTIO_MAX_QUEUE_SIZE;

use core::result::Result;
use alloc::vec::Vec;
use alloc::collections::VecDeque;
use alloc::boxed::Box;
use core::mem;
use core::ops::Deref;
use core::cell::RefCell;
use alloc::rc::Rc;

use drivers::virtio::env::memory::{MemLen, MemOff};
use drivers::virtio::transport::pci::{UniCapsColl, ComCfg, ShMemCfg, NotifCfg, IsrStatus, PciCfgAlt, PciCap};
use drivers::virtio::transport::pci;
use drivers::virtio::driver::VirtioDriver;
use drivers::virtio::error::VirtioError;
use drivers::virtio::virtqueue::{Virtq, VqType, VqSize, VqIndex, BuffSpec, BufferToken, TransferToken, Transfer, Bytes};

use self::error::VirtioNetError;
use self::constants::{Features, Status, FeatureSet, MAX_NUM_VQ};


#[repr(C)]
struct VirtioNetHdr{
    flags: u8,
    gso_type: u8,
    hdr_len: u16,
    csum_start: u16,
    csum_offset: u16,
    num_buffer: u16,
}

/// A wrapper struct for the raw configuration structure. 
/// Handling the right access to fields, as some are read-only
/// for the driver.
struct NetDevCfg {
    raw: &'static NetDevCfgRaw,
    dev_id: u16,

    // Feature booleans
    features: FeatureSet,
}

/// Virtio's network device configuration structure. 
/// See specification v1.1. - 5.1.4
///
#[repr(C)]
struct NetDevCfgRaw {
	mac: [u8; 6],
	status: u16,
	max_virtqueue_pairs: u16,
	mtu: u16,
}

struct CtrlQueue(Option<Rc<Virtq>>);

struct RxQueues {
    vqs: Vec<Rc<Virtq>>,
    poll_queue: Rc<RefCell<VecDeque<Transfer>>>
}

impl RxQueues {
    /// Adds a given queue to the underlying vector and populates the queue with RecvBuffers.
    fn add(&mut self, vq: Virtq, dev_cfg: &NetDevCfg) {
        // Safe virtqueue
        self.vqs.push(Rc::new(vq));
        // Unwrapping is safe, as one virtq will be definitely in the vector.
        let vq = self.vqs.get(self.vqs.len()-1).unwrap();

        if dev_cfg.features.is_feature(Features::VIRTIO_NET_F_GUEST_TSO4) 
            | dev_cfg.features.is_feature(Features::VIRTIO_NET_F_GUEST_TSO6)
            | dev_cfg.features.is_feature(Features::VIRTIO_NET_F_GUEST_UFO) {
            // Receive Buffers must be at least 65562 bytes large with theses features set.
            // See Virtio specification v1.1 - 5.1.6.3.1

            // Buffers can be merged upon receiption, hence using multiple descriptors
            // per buffer. 
            if dev_cfg.features.is_feature(Features::VIRTIO_NET_F_MRG_RXBUF) {
                match u16::from(vq.size()) {
                    // Currently we choose indirect descriptors for small queue sizes, in order to allow 
                    // as many packages as possible inside the queue, while the allocated
                    size if size <= 256 => {
                        let desc_sizes = [Bytes::new(512).unwrap();17];
                        let spec = BuffSpec::Indirect(&desc_sizes);

                        for _ in 0..size {
                            let buff_tkn = match vq.prep_buffer(Rc::clone(vq), None, Some(spec.clone())) {
                                Ok(tkn) => tkn,
                                Err(vq_err) => {
                                    error!("Setup of network queue failed, which should not happen!");
                                    panic!("setup of network queue failed!");
                                }
                            };

                            // BufferTokens are directly provided to the queue
                            // TransferTokens are directly dispatched
                            // Transfers will be awaited at the queue
                            buff_tkn.provide()
                                .dispatch_await(Rc::clone(&self.poll_queue));
                        }
                    },
                    // For queues with a size larger than 256 we choose multiple descriptors inside the actua 
                    // virtqueue. This is duet to not consuming to much memory.
                    // If the queue_size mod 17 is not zero. We use the rest of the descriptors with indirect
                    // descriptors list, to fully utilize the queue.
                    size if size > 256 => {
                        let num_chains = size / 17;
                        let num_indirect = size % 17;
                        // Typically must assert that the size of the virtqueue is not exceeded
                        assert!(size == num_chains*17+num_indirect);

                        // Create all next lists
                        let desc_sizes = [Bytes::new(512).unwrap();17];
                        let spec = BuffSpec::Multiple(&desc_sizes);

                        for _ in 0..num_chains {
                            let buff_tkn = match vq.prep_buffer(Rc::clone(vq), None, Some(spec.clone())) {
                                Ok(tkn) => tkn,
                                Err(vq_err) => {
                                    error!("Setup of network queue failed, which should not happen!");
                                    panic!("setup of network queue failed!");
                                }
                            };

                            // BufferTokens are directly provided to the queue
                            // TransferTokens are directly dispatched
                            // Transfers will be awaited at the queue
                            buff_tkn.provide()
                                .dispatch_await(Rc::clone(&self.poll_queue));
                        }

                        // Create remaining indirect descriptors
                        let spec = BuffSpec::Indirect(&desc_sizes);
                        for _ in 0..num_indirect {
                            let buff_tkn = match vq.prep_buffer(Rc::clone(vq), None, Some(spec.clone())) {
                                Ok(tkn) => tkn,
                                Err(vq_err) => {
                                    error!("Setup of network queue failed, which should not happen!");
                                    panic!("setup of network queue failed!");
                                }
                            };

                            // BufferTokens are directly provided to the queue
                            // TransferTokens are directly dispatched
                            // Transfers will be awaited at the queue
                            buff_tkn.provide()
                                .dispatch_await(Rc::clone(&self.poll_queue));
                        }
                    },
                    _ => unreachable!(),
                }
            } else {
            // Buffers can not be merged, hence using a single descriptor per buffer.
                let spec = BuffSpec::Single(Bytes::new(65562).unwrap());
                for _ in 0..u16::from(vq.size()) {
                    let buff_tkn = match vq.prep_buffer(Rc::clone(vq), None, Some(spec.clone())) {
                        Ok(tkn) => tkn,
                        Err(vq_err) => {
                            error!("Setup of network queue failed, which should not happen!");
                            panic!("setup of network queue failed!");
                        }
                    };

                    // BufferTokens are directly provided to the queue
                    // TransferTokens are directly dispatched
                    // Transfers will be awaited at the queue
                    buff_tkn.provide()
                        .dispatch_await(Rc::clone(&self.poll_queue));
                }  
            }
        } else {
            // If above features not set, buffers must be at least 1526 bytes large.
            // See Virtio specification v1.1 - 5.1.6.3.1
            //
            // In this case, the driver does not check if 
            // VIRTIO_NET_F_MRG_RXBUF is set, as a single descriptor will be used anyway.
            let spec = BuffSpec::Single(Bytes::new(1526usize).unwrap());
            for _ in 0..u16::from(vq.size()) {
                let buff_tkn = match vq.prep_buffer(Rc::clone(vq), None, Some(spec.clone())) {
                    Ok(tkn) => tkn,
                    Err(vq_err) => {
                        error!("Setup of network queue failed, which should not happen!");
                        panic!("setup of network queue failed!");
                    }
                };

                // BufferTokens are directly provided to the queue
                // TransferTokens are directly dispatched
                // Transfers will be awaited at the queue
                buff_tkn.provide()
                    .dispatch_await(Rc::clone(&self.poll_queue));
            } 
        }
    }
}

struct TxQueues { 
    vqs: Vec<Rc<Virtq>>,
    poll_queue: Rc<RefCell<VecDeque<Transfer>>>,
} 

impl TxQueues {
    fn add(&mut self, vq: Virtq, dev_cfg: &NetDevCfg) {
        todo!();
    } 
}

struct RxBuffer {

}

struct TxBuffer {

}

/// Virtio network driver struct. 
///
/// Struct allows to control devices virtqueues as also
/// the device itself.
pub struct VirtioNetDriver{
    dev_cfg: NetDevCfg,
    com_cfg: ComCfg,
    isr_stat: IsrStatus,
    notif_cfg: NotifCfg,

    ctrl_vq: CtrlQueue,
    recv_vqs: RxQueues, 
    send_vqs: TxQueues,

    num_vqs: u16,
}

impl VirtioDriver for VirtioNetDriver {
    fn add_buff(&self) {
        unimplemented!();
    }

    fn get_buff(&self) {
        unimplemented!();
    }

    fn process_buff(&self) {
        unimplemented!();
    }

    fn set_notif(&self){
        unimplemented!();
    }
}

// Private funtctions for Virtio network driver
impl VirtioNetDriver {
    fn map_cfg(cap: &PciCap) -> Option<NetDevCfg> {
        if cap.bar_len() <  u64::from(cap.len() + cap.offset()) {
            error!("Network config of device {:x}, does not fit into memeory specified by bar!", 
                cap.dev_id(),
            );
            return None
        }

        // Drivers MAY do this check. See Virtio specification v1.1. - 4.1.4.1
        if cap.len() < MemLen::from(mem::size_of::<NetDevCfg>()*8) {
            error!("Network config from device {:x}, does not represent actual structure specified by the standard!", cap.dev_id());
            return None 
        }

        let virt_addr_raw = cap.bar_addr() + cap.offset();

        // Create mutable reference to the PCI structure in PCI memory
        let dev_cfg: &mut NetDevCfgRaw = unsafe {
            &mut *(usize::from(virt_addr_raw) as *mut NetDevCfgRaw)
        };

        Some(NetDevCfg {
            raw: dev_cfg,
            dev_id: cap.dev_id(),
            features: FeatureSet::new(0),
        })
    }

    /// Instanciates a new (VirtioNetDriver)[VirtioNetDriver] struct, by checking the available 
    /// configuration structures and moving them into the struct.
    fn new(mut caps_coll: UniCapsColl, adapter: &PciAdapter) -> Result<Self, error::VirtioNetError> {
        let com_cfg =  loop { 
            match caps_coll.get_com_cfg() {
                Some(com_cfg) => break com_cfg,
                None => return Err(error::VirtioNetError::NoComCfg(adapter.device_id)),
            }
        };

        let isr_stat = loop {
            match caps_coll.get_isr_cfg(){
                Some(isr_stat) => break isr_stat,
                None => return Err(error::VirtioNetError::NoIsrCfg(adapter.device_id)),
            }
        };

        let notif_cfg = loop {
            match caps_coll.get_notif_cfg() {
                Some(notif_cfg) => break notif_cfg,
                None => return Err(error::VirtioNetError::NoNotifCfg(adapter.device_id)),
            }
        };

        let dev_cfg = loop {
            match caps_coll.get_dev_cfg() {
                Some(cfg) => { 
                    match VirtioNetDriver::map_cfg(&cfg) {
                        Some(dev_cfg) => break dev_cfg,
                        None => (),
                    }
                },
                None => return Err(error::VirtioNetError::NoDevCfg(adapter.device_id)),
            }
        };

        Ok(VirtioNetDriver {
            dev_cfg,
            com_cfg,
            isr_stat,
            notif_cfg,

            ctrl_vq: CtrlQueue(None),
            recv_vqs: RxQueues {
                vqs: Vec::<Rc<Virtq>>::new(),
                poll_queue: Rc::new(RefCell::new(VecDeque::new())),
            },
            send_vqs: TxQueues {
                vqs: Vec::<Rc<Virtq>>::new(),
                poll_queue: Rc::new(RefCell::new(VecDeque::new())),
            },
            num_vqs: 0,
        })
    }

    /// Initallizes the device in adherence to specificaton. Returns Some(VirtioNetError)
    /// upon failure and None in case everything worked as expected.
    ///
    /// See Virtio specification v1.1. - 3.1.1. 
    ///                      and v1.1. - 5.1.5
    fn init_dev(&mut self) -> Result<(), VirtioNetError> {
        // Reset
        self.com_cfg.reset_dev();

        // Indiacte device, that OS noticed it
        self.com_cfg.ack_dev();

        // Indicate device, that driver is able to handle it 
        self.com_cfg.set_drv();

        // Define minimal feature set
        let min_feats: Vec<Features>  = vec![Features::VIRTIO_F_VERSION_1,
            Features::VIRTIO_NET_F_GUEST_CSUM,
            Features::VIRTIO_NET_F_MAC, 
            Features::VIRTIO_NET_F_STATUS,
            Features::VIRTIO_NET_F_GUEST_TSO4,
            Features::VIRTIO_NET_F_GUEST_TSO6,
        ];

        let mut min_feat_set = FeatureSet::new(0);
        min_feat_set.set_features(&min_feats);
        let mut feats: Vec<Features> = Vec::from(min_feats);
 
        // If wanted, push new features into feats here:
        // 
        // Merging RxBuffers is possible and wanted
        feats.push(Features::VIRTIO_NET_F_MRG_RXBUF);

        // Negotiate features with device. Automatically reduces selected feats in order to meet device capabilites.
        // Aborts in case incompatible features are selected by the dricer or the device does not support min_feat_set.
        match self.negotiate_features(&feats) {
            Ok(_) => info!("Driver found a subset of features for virtio device {:x}. Features are: {:?}", self.dev_cfg.dev_id, &feats),
            Err(vnet_err) => {
                match vnet_err {
                    VirtioNetError::FeatReqNotMet(feat_set) => {
                        error!("Network drivers feature set {:x} does not satisfy rules in section 5.1.3.1 of specification v1.1. Aborting!", u64::from(feat_set));
                        return Err(vnet_err);
                    },
                    VirtioNetError::IncompFeatsSet(drv_feats, dev_feats) => {
                        // Create a new matching feature set for device and driver if the minimal set is met!
                        if (min_feat_set & drv_feats) != min_feat_set {
                            return Err(VirtioNetError::FailFeatureNeg(self.dev_cfg.dev_id))
                        } else {
                            feats = match Features::into_features(dev_feats & drv_feats) {
                                Some(feats) => feats,
                                None => return Err(VirtioNetError::FailFeatureNeg(self.dev_cfg.dev_id))
                            };
                        }

                        match self.negotiate_features(&feats) {
                            Ok(_) => info!("Driver found a subset of features for virtio device {:x}. Features are: {:?}", self.dev_cfg.dev_id, &feats),
                            Err(vnet_err) => {
                                match vnet_err {
                                    VirtioNetError::FeatReqNotMet(feat_set) => {
                                        error!("Network device offers a feature set {:x} when used completly does not satisfy rules in section 5.1.3.1 of specification v1.1. Aborting!", u64::from(feat_set));
                                        return Err(vnet_err)
                                    },
                                    _ => {
                                        error!("Feature Set after reduction still not usable. Set: {:?}. Aborting!", feats);
                                        return Err(vnet_err) 
                                    }
                                }
                            }
                        }
                    },
                    _ => {
                        error!("Wanted set of features is NOT supported by device. Set: {:?}", feats);
                        return Err(vnet_err)
                    },
                }
            },
        }
        
        // Indicates the device, that the current feature set is final for the driver
        // and will not be changed.
        self.com_cfg.features_ok();

        // Checks if the device has accepted final set. This finishes feature negotiation.
        if self.com_cfg.check_features() {
            info!("Features have been negotiated between virtio network device {:x} and driver.", self.dev_cfg.dev_id);
            // Set feature set in device config fur future use.
            self.dev_cfg.features.set_features(&feats);
        } else {
            return Err(VirtioNetError::FailFeatureNeg(self.dev_cfg.dev_id));
        }

        match self.dev_spec_init() {
            Ok(_) => info!("Device specific initalization for Virtio network defice {:x} finished", self.dev_cfg.dev_id),
            Err(vnet_err) => return Err(vnet_err),
        }

        // At this point the device is "live"
        self.com_cfg.drv_ok();

        Ok(())
    }

    /// Negotiates a subset of features, understood and wanted by both the OS 
    /// and the device.
    fn negotiate_features(&mut self, wanted_feats: &Vec<Features>) -> Result<(), VirtioNetError> {
        let mut drv_feats = FeatureSet::new(0);
        
        for feat in wanted_feats.iter() {
            drv_feats |= *feat;
        }

        let dev_feats = FeatureSet::new(self.com_cfg.dev_features());

        
        // Checks if the selected feature set is compatible with requirements for 
        // features according to Virtio spec. v1.1 - 5.1.3.1.
        match FeatureSet::check_features(&wanted_feats) {
            Ok(_) => info!("Feature set wanted by network driver, matches virtio netword devices capabiliites."),
            Err(vnet_err) => return Err(vnet_err),
        }

        if (dev_feats & drv_feats) == drv_feats {
            // If device supports subset of features write feature set to common config
            self.com_cfg.set_drv_features(drv_feats.into());
            Ok(())
        } else {
            Err(VirtioNetError::IncompFeatsSet(drv_feats, dev_feats))
        }
    }

    /// Device Specfic initalization according to Virtio specifictation v1.1. - 5.1.5
    fn dev_spec_init(&mut self) -> Result<(), VirtioNetError> {
        match self.virtqueue_init() {
            Ok(_) => info!("Network driver successfully initalized virtqueues."),
            Err(vnet_err) => return Err(vnet_err),
        }

        // Add a control if feature is negotiated
        if self.dev_cfg.features.is_feature(Features::VIRTIO_NET_F_CTRL_VQ) {
            if self.dev_cfg.features.is_feature(Features::VIRTIO_F_RING_PACKED) {
                self.ctrl_vq = CtrlQueue(Some(Rc::new(Virtq::new(&mut self.com_cfg,
              VqSize::from(VIRTIO_MAX_QUEUE_SIZE),
                    VqType::Packed, 
             VqIndex::from(2*self.num_vqs+1)
                ))));
            } else {
                todo!("Implement control queue for split queue")
            }
        }

        // If device does not take care of MAC address, the driver has to create one
        if !self.dev_cfg.features.is_feature(Features::VIRTIO_NET_F_MAC) {
            todo!("Driver created MAC address should be passed to device here.")
        }

        Ok(())
    }

    /// Initalize virtqueues via the queue interface and populates receiving queues
    fn virtqueue_init(&mut self) -> Result<(), VirtioNetError> {
        // We are assuming here, that the device single source of truth is the 
        // device specific configuration. Hence we do NOT check if
        // 
        // max_virtqueue_pairs + 1 < num_queues
        //
        // - the plus 1 is due to the possibility of an exisiting control queue
        // - the num_queues is found in the ComCfg struct of the device and defines the maximal number 
        // of supported queues.
        if self.dev_cfg.features.is_feature(Features::VIRTIO_NET_F_MQ) {
            if self.dev_cfg.raw.max_virtqueue_pairs <= MAX_NUM_VQ {
                self.num_vqs = MAX_NUM_VQ;
            } else {
                self.num_vqs = self.dev_cfg.raw.max_virtqueue_pairs;
            }
        } else {
            // Minimal number of virtqueues defined in the standard v1.1. - 5.1.5 Step 1
            self.num_vqs = 2;
        }

        // The loop is running from 1 to num_vqs+1 and the indexes are provided to the VqIndex::from function in this way 
        // in order to allow the indexes of the queues to be in a form of: 
        //
        // index i for receiv queue 
        // index i+1 for send queue
        //
        // as it is wanted by the network network device. 
        // see Virtio specification v1.1. - 5.1.2 
        for i in 1..self.num_vqs+1 {
            if self.dev_cfg.features.is_feature(Features::VIRTIO_F_RING_PACKED) {
                let vq = Virtq::new(&mut self.com_cfg,
                 VqSize::from(VIRTIO_MAX_QUEUE_SIZE), 
                       VqType::Packed, 
                VqIndex::from(2*i-1)
                );
                self.recv_vqs.add(vq, &self.dev_cfg);
        
                let vq = Virtq::new(&mut self.com_cfg,
              VqSize::from(VIRTIO_MAX_QUEUE_SIZE),
                    VqType::Packed, 
             VqIndex::from(2*i)
                );
                self.send_vqs.add(vq, &self.dev_cfg);
            } else {
                todo!("Integrate split virtqueue into network driver");
            }
        }
        Ok(())
    }
}

// Public interface for virtio network driver.
impl VirtioNetDriver { 
    /// Initializes virtio network device by mapping configuration layout to 
    /// respective structs (configuration structs are:
    /// [ComCfg](structs.comcfg.html), [NotifCfg](structs.notifcfg.html)
    /// [IsrStatus](structs.isrstatus.html), [PciCfg](structs.pcicfg.html)
    /// [ShMemCfg](structs.ShMemCfg)). 
    ///
    /// Returns a driver instance of 
    /// [VirtioNetDriver](structs.virtionetdriver.html) or an [VirtioError](enums.virtioerror.html).
    pub fn init(adapter: &PciAdapter) -> Result<VirtioNetDriver, VirtioError> {
        let mut drv = match pci::map_caps(adapter) {
            Ok(caps) => match VirtioNetDriver::new(caps, adapter) {
                Ok(driver) => driver,
                Err(vnet_err) => return Err(VirtioError::NetDriver(vnet_err)),
            },
            Err(pci_error) => return Err(VirtioError::FromPci(pci_error)),
        };

        match drv.init_dev() {
            Ok(_) => info!("Network device with id {:x}, has been initalized by driver!", drv.dev_cfg.dev_id),
            Err(vnet_err) => {
                drv.com_cfg.set_failed();
                return Err(VirtioError::NetDriver(vnet_err))
            },
        }

        if drv.dev_status() & u16::from(Status::VIRTIO_NET_S_LINK_UP) == u16::from(Status::VIRTIO_NET_S_LINK_UP) {
            info!("Virtio-net link is up after initalization.")
        } else {
            info!("Virtio-net link is down after initalization!")
        }

        Ok(drv)
    }

    pub fn dev_status(&self) -> u16 {
        todo!("Check if check for status feature bit is necessary here");
        self.dev_cfg.raw.status
    }
}

mod constants {
    use core::ops::{BitOr, BitOrAssign, BitAnd, BitAndAssign};
    use core::fmt::Display;
    use super::error::VirtioNetError; 
    use alloc::vec::Vec;

    // Configuration constants
    pub const MAX_NUM_VQ:u16 = 2;
    
    /// Enum contains virtio's network device features and general features of Virtio.
    ///
    /// See Virtio specification v1.1. - 5.1.3
    /// 
    /// See Virtio specification v1.1. - 6
    //
    // WARN: In case the enum is changed, the static function of features `into_features(feat: u64) -> 
    // Option<Vec<Features>>` must also be adjusted to return a corret vector of features.
    #[allow(dead_code, non_camel_case_types)]
    #[derive(Copy, Clone, Debug)]
    #[repr(u64)]
    pub enum Features {
        VIRTIO_NET_F_CSUM = 1 << 0,
        VIRTIO_NET_F_GUEST_CSUM = 1 << 1,
        VIRTIO_NET_F_CTRL_GUEST_OFFLOADS = 1 << 2,
        VIRTIO_NET_F_MTU = 1 << 3, 
        VIRTIO_NET_F_MAC = 1 << 5,
        VIRTIO_NET_F_GUEST_TSO4 = 1 << 7,
        VIRTIO_NET_F_GUEST_TSO6 = 1 << 8,
        VIRTIO_NET_F_GUEST_ECN = 1 <<  9,
        VIRTIO_NET_F_GUEST_UFO = 1 <<  10,
        VIRTIO_NET_F_HOST_TSO4 = 1 <<  11,
        VIRTIO_NET_F_HOST_TSO6 = 1 <<  12,
        VIRTIO_NET_F_HOST_ECN = 1 <<  13,
        VIRTIO_NET_F_HOST_UFO = 1 <<  14,
        VIRTIO_NET_F_MRG_RXBUF = 1 <<  15,
        VIRTIO_NET_F_STATUS = 1 <<  16,
        VIRTIO_NET_F_CTRL_VQ = 1 <<  17,
        VIRTIO_NET_F_CTRL_RX = 1 <<  18,
        VIRTIO_NET_F_CTRL_VLAN = 1 << 19,
        VIRTIO_NET_F_GUEST_ANNOUNCE = 1 << 21,
        VIRTIO_NET_F_MQ = 1 << 22,
        VIRTIO_NET_F_CTRL_MAC_ADDR = 1 << 23,
        VIRTIO_F_RING_INDIRECT_DESC = 1 << 28,
	    VIRTIO_F_RING_EVENT_IDX = 1 << 29,
	    VIRTIO_F_VERSION_1 = 1 << 32,
	    VIRTIO_F_ACCESS_PLATFORM = 1 << 33,
	    VIRTIO_F_RING_PACKED = 1 << 34,
	    VIRTIO_F_IN_ORDER = 1 << 35,
	    VIRTIO_F_ORDER_PLATFORM = 1 << 36,
	    VIRTIO_F_SR_IOV = 1 << 37,
	    VIRTIO_F_NOTIFICATION_DATA = 1 << 38,
        VIRTIO_NET_F_GUEST_HDRLEN = 1 << 59,
        VIRTIO_NET_F_RSC_EXT = 1 << 61,
        VIRTIO_NET_F_STANDBY = 1 << 62,

        // INTERNAL DOCUMENTATION TO KNOW WHICH FEATURES HAVE REQUIREMENTS
        // 
        // 5.1.3.1 Feature bit requirements
        // Some networking feature bits require other networking feature bits (see 2.2.1): 
        // VIRTIO_NET_F_GUEST_TSO4 Requires VIRTIO_NET_F_GUEST_CSUM.
        // VIRTIO_NET_F_GUEST_TSO6 Requires VIRTIO_NET_F_GUEST_CSUM.
        // VIRTIO_NET_F_GUEST_ECN Requires VIRTIO_NET_F_GUEST_TSO4orVIRTIO_NET_F_GUEST_TSO6.
        // VIRTIO_NET_F_GUEST_UFO Requires VIRTIO_NET_F_GUEST_CSUM.
        // VIRTIO_NET_F_HOST_TSO4 Requires VIRTIO_NET_F_CSUM.
        // VIRTIO_NET_F_HOST_TSO6 Requires VIRTIO_NET_F_CSUM.
        // VIRTIO_NET_F_HOST_ECN Requires VIRTIO_NET_F_HOST_TSO4 or VIRTIO_NET_F_HOST_TSO6.
        // VIRTIO_NET_F_HOST_UFO Requires VIRTIO_NET_F_CSUM.
        // VIRTIO_NET_F_CTRL_RX Requires VIRTIO_NET_F_CTRL_VQ.
        // VIRTIO_NET_F_CTRL_VLAN Requires VIRTIO_NET_F_CTRL_VQ. 
        // VIRTIO_NET_F_GUEST_ANNOUNCE Requires VIRTIO_NET_F_CTRL_VQ.
        // VIRTIO_NET_F_MQ Requires VIRTIO_NET_F_CTRL_VQ.
        // VIRTIO_NET_F_CTRL_MAC_ADDR Requires VIRTIO_NET_F_CTRL_VQ.
        // VIRTIO_NET_F_RSC_EXT Requires VIRTIO_NET_F_HOST_TSO4 or VIRTIO_NET_F_HOST_TSO6.
    }

    impl From<Features> for u64 {
        fn from(val: Features) -> Self {
           match val {
            Features::VIRTIO_NET_F_CSUM => 1 << 0,
            Features::VIRTIO_NET_F_GUEST_CSUM => 1 << 1,
            Features::VIRTIO_NET_F_CTRL_GUEST_OFFLOADS => 1 << 2,
            Features::VIRTIO_NET_F_MTU => 1 << 3,
            Features::VIRTIO_NET_F_MAC => 1 << 5,
            Features::VIRTIO_NET_F_GUEST_TSO4 => 1 << 7,
            Features::VIRTIO_NET_F_GUEST_TSO6 => 1 << 8,
            Features::VIRTIO_NET_F_GUEST_ECN => 1 <<  9,
            Features::VIRTIO_NET_F_GUEST_UFO => 1 <<  10,
            Features::VIRTIO_NET_F_HOST_TSO4 => 1 <<  11,
            Features::VIRTIO_NET_F_HOST_TSO6 => 1 <<  12,
            Features::VIRTIO_NET_F_HOST_ECN => 1 <<  13,
            Features::VIRTIO_NET_F_HOST_UFO => 1 <<  14,
            Features::VIRTIO_NET_F_MRG_RXBUF => 1 <<  15,
            Features::VIRTIO_NET_F_STATUS => 1 <<  16,
            Features::VIRTIO_NET_F_CTRL_VQ => 1 <<  17,
            Features::VIRTIO_NET_F_CTRL_RX => 1 <<  18,
            Features::VIRTIO_NET_F_CTRL_VLAN => 1 << 19,
            Features::VIRTIO_NET_F_GUEST_ANNOUNCE => 1 << 21,
            Features::VIRTIO_NET_F_MQ => 1 << 22,
            Features::VIRTIO_NET_F_CTRL_MAC_ADDR => 1 << 23,
            Features::VIRTIO_F_RING_INDIRECT_DESC => 1 << 28,
            Features::VIRTIO_F_RING_EVENT_IDX => 1 << 29,
            Features::VIRTIO_F_VERSION_1 => 1 << 32,
            Features::VIRTIO_F_ACCESS_PLATFORM => 1 << 33,
            Features::VIRTIO_F_RING_PACKED => 1 << 34,
            Features::VIRTIO_F_IN_ORDER => 1 << 35,
            Features::VIRTIO_F_ORDER_PLATFORM => 1 << 36,
            Features::VIRTIO_F_SR_IOV => 1 << 37,
            Features::VIRTIO_F_NOTIFICATION_DATA => 1 << 38,
            Features::VIRTIO_NET_F_GUEST_HDRLEN => 1 << 59,
            Features::VIRTIO_NET_F_RSC_EXT => 1 << 61,
            Features::VIRTIO_NET_F_STANDBY => 1 << 62,
           } 
        }
    }

    impl BitOr for Features {
        type Output = u64;

        fn bitor(self, rhs: Self) -> Self::Output {
           u64::from(self) | u64::from(rhs) 
        }
    }

    impl BitOr<Features> for u64 {
        type Output = u64;

        fn bitor(self, rhs: Features) -> Self::Output {
            self | u64::from(rhs)
        }
    }

    impl BitOrAssign<Features> for u64 {
        fn bitor_assign(&mut self, rhs: Features) {
            *self |= u64::from(rhs);
        }
    }

    impl BitAnd for Features {
        type Output = u64; 

        fn bitand(self, rhs: Features) -> Self::Output {
            u64::from(self) & u64::from(rhs)
        }
    }

    impl BitAnd<Features> for u64 {
        type Output = u64;

        fn bitand(self, rhs: Features) -> Self::Output {
            self & u64::from(rhs)
        }
    }

    impl BitAndAssign<Features> for u64 {
        fn bitand_assign(&mut self, rhs: Features) {
            *self &= u64::from(rhs);
        }
    }

    impl core::fmt::Display for Features {
        fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
            match *self {
                Features::VIRTIO_NET_F_CSUM => write!(f,"VIRTIO_NET_F_CSUM"),
                Features::VIRTIO_NET_F_GUEST_CSUM => write!(f,"VIRTIO_NET_F_GUEST_CSUM"),
                Features::VIRTIO_NET_F_CTRL_GUEST_OFFLOADS => write!(f,"VIRTIO_NET_F_CTRL_GUEST_OFFLOADS"),
                Features::VIRTIO_NET_F_MTU => write!(f,"VIRTIO_NET_F_MTU"),
                Features::VIRTIO_NET_F_MAC => write!(f,"VIRTIO_NET_F_MAC"),
                Features::VIRTIO_NET_F_GUEST_TSO4 => write!(f,"VIRTIO_NET_F_GUEST_TSO4"),
                Features::VIRTIO_NET_F_GUEST_TSO6 => write!(f,"VIRTIO_NET_F_GUEST_TSO6"),
                Features::VIRTIO_NET_F_GUEST_ECN => write!(f,"VIRTIO_NET_F_GUEST_ECN"),
                Features::VIRTIO_NET_F_GUEST_UFO => write!(f,"VIRTIO_NET_FGUEST_UFO"),
                Features::VIRTIO_NET_F_HOST_TSO4 => write!(f,"VIRTIO_NET_F_HOST_TSO4"),
                Features::VIRTIO_NET_F_HOST_TSO6 => write!(f,"VIRTIO_NET_F_HOST_TSO6"),
                Features::VIRTIO_NET_F_HOST_ECN => write!(f,"VIRTIO_NET_F_HOST_ECN"),
                Features::VIRTIO_NET_F_HOST_UFO => write!(f,"VIRTIO_NET_F_HOST_UFO"),
                Features::VIRTIO_NET_F_MRG_RXBUF => write!(f,"VIRTIO_NET_F_MRG_RXBUF"),
                Features::VIRTIO_NET_F_STATUS => write!(f,"VIRTIO_NET_F_STATUS"),
                Features::VIRTIO_NET_F_CTRL_VQ => write!(f,"VIRTIO_NET_F_CTRL_VQ"),
                Features::VIRTIO_NET_F_CTRL_RX => write!(f,"VIRTIO_NET_F_CTRL_RX"),
                Features::VIRTIO_NET_F_CTRL_VLAN => write!(f,"VIRTIO_NET_F_CTRL_VLAN"),
                Features::VIRTIO_NET_F_GUEST_ANNOUNCE => write!(f,"VIRTIO_NET_F_GUEST_ANNOUNCE"),
                Features::VIRTIO_NET_F_MQ => write!(f,"VIRTIO_NET_F_MQ"),
                Features::VIRTIO_NET_F_CTRL_MAC_ADDR => write!(f,"VIRTIO_NET_F_CTRL_MAC_ADDR"),
                Features::VIRTIO_F_RING_INDIRECT_DESC => write!(f,"VIRTIO_F_RING_INDIRECT_DESC"),
                Features::VIRTIO_F_RING_EVENT_IDX => write!(f,"VIRTIO_F_RING_EVENT_IDX"),
                Features::VIRTIO_F_VERSION_1 => write!(f,"VIRTIO_F_VERSION_1"),
                Features::VIRTIO_F_ACCESS_PLATFORM => write!(f,"VIRTIO_F_ACCESS_PLATFORM"),
                Features::VIRTIO_F_RING_PACKED => write!(f,"VIRTIO_F_RING_PACKED"),
                Features::VIRTIO_F_IN_ORDER => write!(f,"VIRTIO_F_IN_ORDER"),
                Features::VIRTIO_F_ORDER_PLATFORM => write!(f,"VIRTIO_F_ORDER_PLATFORM"),
                Features::VIRTIO_F_SR_IOV => write!(f,"VIRTIO_F_SR_IOV"),
                Features::VIRTIO_F_NOTIFICATION_DATA => write!(f,"VIRTIO_F_NOTIFICATION_DATA"),
                Features::VIRTIO_NET_F_GUEST_HDRLEN => write!(f,"VIRTIO_NET_F_GUEST_HDRLEN"),
                Features::VIRTIO_NET_F_RSC_EXT => write!(f,"VIRTIO_NET_F_RSC_EXT"),
                Features::VIRTIO_NET_F_STANDBY => write!(f,"VIRTIO_NET_F_STANDBY"),
            }
        }
    }

    impl Features {
        /// Return a vector of [Features](Features) for a given input of a u64 representation.
        ///
        /// INFO: In case the FEATURES enum is changed, this function MUST also be adjusted to the new set!
        //
        // Really UGLY function, but currently the most convenienvt one to reduce the set of features for the driver easily!
        pub fn into_features(feat_set: FeatureSet) -> Option<Vec<Features>> {
            let mut vec_of_feats: Vec<Features> = Vec::new();
            let feats = feat_set.0;

           if feats & (1 << 0) != 0 {
            vec_of_feats.push(Features::VIRTIO_NET_F_CSUM)
           }
           if feats & (1 << 1) != 0 {
            vec_of_feats.push(Features::VIRTIO_NET_F_GUEST_CSUM)
           }
           if feats & (1 << 2) != 0 {
            vec_of_feats.push(Features::VIRTIO_NET_F_CTRL_GUEST_OFFLOADS)
           }
           if feats & (1 << 3) != 0 {
            vec_of_feats.push(Features::VIRTIO_NET_F_MTU)
           }
           if feats & (1 << 5) != 0 {
            vec_of_feats.push(Features::VIRTIO_NET_F_MAC)
           }
           if feats & (1 << 7) != 0 {
            vec_of_feats.push(Features::VIRTIO_NET_F_GUEST_TSO4)
           }
           if feats & (1 << 8) != 0 {
            vec_of_feats.push(Features::VIRTIO_NET_F_GUEST_TSO6)
           }
           if feats & (1 << 9) != 0 {
            vec_of_feats.push(Features::VIRTIO_NET_F_GUEST_ECN)
           }
           if feats & (1 << 10) != 0 {
            vec_of_feats.push(Features::VIRTIO_NET_F_GUEST_UFO)
           }
           if feats & (1 << 11) != 0 {
            vec_of_feats.push(Features::VIRTIO_NET_F_HOST_TSO4)
           }
           if feats & (1 << 12) != 0 {
            vec_of_feats.push(Features::VIRTIO_NET_F_HOST_TSO6)
           }
           if feats & (1 << 13) != 0 {
            vec_of_feats.push(Features::VIRTIO_NET_F_HOST_ECN)
           }
           if feats & (1 << 14) != 0 {
            vec_of_feats.push(Features::VIRTIO_NET_F_HOST_UFO)
           }
           if feats & (1 << 15) != 0 {
            vec_of_feats.push(Features::VIRTIO_NET_F_MRG_RXBUF)
           }
           if feats & (1 << 16) != 0 {
            vec_of_feats.push(Features::VIRTIO_NET_F_STATUS)
           }
           if feats & (1 << 17) != 0 {
            vec_of_feats.push(Features::VIRTIO_NET_F_CTRL_VQ)
           }
           if feats & (1 << 18) != 0 {
            vec_of_feats.push(Features::VIRTIO_NET_F_CTRL_RX)
           }
           if feats & (1 << 19) != 0 {
            vec_of_feats.push(Features::VIRTIO_NET_F_CTRL_VLAN)
           }
           if feats & (1 << 21) != 0 {
            vec_of_feats.push(Features::VIRTIO_NET_F_GUEST_ANNOUNCE)
           }
           if feats & (1 << 22) != 0 {
            vec_of_feats.push(Features::VIRTIO_NET_F_MQ)
           }
           if feats & (1 << 23) != 0 {
            vec_of_feats.push(Features::VIRTIO_NET_F_CTRL_MAC_ADDR)
           }
           if feats & (1 << 28) != 0 {
            vec_of_feats.push(Features::VIRTIO_F_RING_INDIRECT_DESC)
           }
           if feats & (1 << 29) != 0 {
            vec_of_feats.push(Features::VIRTIO_F_RING_EVENT_IDX)
           }
           if feats & (1 << 32) != 0 {
            vec_of_feats.push(Features::VIRTIO_F_VERSION_1)
           }
           if feats & (1 << 33) != 0 {
            vec_of_feats.push(Features::VIRTIO_F_ACCESS_PLATFORM)
           }
           if feats & (1 << 34) != 0 {
            vec_of_feats.push(Features::VIRTIO_F_RING_PACKED)
           }
           if feats & (1 << 35) != 0 {
            vec_of_feats.push(Features::VIRTIO_F_IN_ORDER)
           }
           if feats & (1 << 36) != 0 {
            vec_of_feats.push(Features::VIRTIO_F_ORDER_PLATFORM)
           }
           if feats & (1 << 37) != 0 {
            vec_of_feats.push(Features::VIRTIO_F_SR_IOV)
           }
           if feats & (1 << 38) != 0 {
            vec_of_feats.push(Features::VIRTIO_F_NOTIFICATION_DATA)
           }
           if feats & (1 << 59) != 0 {
            vec_of_feats.push(Features::VIRTIO_NET_F_GUEST_HDRLEN)
           }
           if feats & (1 << 61) != 0 {
            vec_of_feats.push(Features::VIRTIO_NET_F_RSC_EXT)
           }
           if feats & (1 << 62) != 0 {
            vec_of_feats.push(Features::VIRTIO_NET_F_STANDBY)
           }

            if vec_of_feats.is_empty() {
                None 
            } else {
                Some(vec_of_feats)
            }
        }
    }

    /// Enum contains virtio's network device status
    /// indiacted in the status field of the device's 
    /// configuration structure.
    ///
    /// See Virtio specification v1.1. - 5.1.4
    #[allow(dead_code, non_camel_case_types)]
    #[derive(Copy, Clone, Debug)]
    #[repr(u16)]
    pub enum Status {
        VIRTIO_NET_S_LINK_UP = 1 << 0,
        VIRTIO_NET_S_ANNOUNCE = 1 << 1,
    }

    impl From<Status> for u16 {
        fn from(stat: Status) -> Self {
            match stat {
                Status::VIRTIO_NET_S_LINK_UP => 1,
                Status::VIRTIO_NET_S_ANNOUNCE => 2,
            }
        }
    }


    /// FeatureSet is new type whicih holds features for virito network devices indicated by the virtio specification 
    /// v1.1. - 5.1.3. and all General Features defined in Virtio specification v1.1. - 6
    /// wrapping a u64. 
    /// 
    /// The main functionality of this type are functions implemented on it.
    #[derive(Debug, Copy, Clone, PartialOrd, PartialEq)]
    pub struct FeatureSet(u64);

    impl BitOr for FeatureSet {
        type Output = FeatureSet;

        fn bitor(self, rhs: Self) -> Self::Output {
           FeatureSet(self.0 | rhs.0)
        }
    }

    impl BitOr<FeatureSet> for u64 {
        type Output = u64;

        fn bitor(self, rhs: FeatureSet) -> Self::Output {
            self | u64::from(rhs)
        }
    }

    impl BitOrAssign<FeatureSet> for u64 {
        fn bitor_assign(&mut self, rhs: FeatureSet) {
            *self |= u64::from(rhs);
        }
    }

    impl BitOrAssign<Features> for FeatureSet {
        fn bitor_assign(&mut self, rhs: Features) {
            self.0 = self.0 | u64::from(rhs);
        }
    }

    impl BitAnd for FeatureSet {
        type Output = FeatureSet; 

        fn bitand(self, rhs: FeatureSet) -> Self::Output {
            FeatureSet(self.0 & rhs.0)
        }
    }

    impl BitAnd<FeatureSet> for u64 {
        type Output = u64;

        fn bitand(self, rhs: FeatureSet) -> Self::Output {
            self & u64::from(rhs)
        }
    }

    impl BitAndAssign<FeatureSet> for u64 {
        fn bitand_assign(&mut self, rhs: FeatureSet) {
            *self &= u64::from(rhs);
        }
    }

    impl From<FeatureSet> for u64 {
        fn from(feature_set: FeatureSet) -> Self {
            feature_set.0
        }
    }

    impl FeatureSet {
        /// Checks if a given set of features is compatible and adheres to the 
        /// specfification v1.1. - 5.1.3.1
        /// Upon an error returns the incompatible set of features by the 
        /// [FeatReqNotMet](self::error::VirtioNetError) errror value, which
        /// wraps the u64 indicating the feature set.
        ///
        /// INFO: Iterates twice over the vector of features.
        pub fn check_features(feats: &Vec<Features>) -> Result<(), VirtioNetError> {
            let mut feat_bits = 0u64;

            for feat in feats.iter() {
                feat_bits |= *feat;
            }

            for feat in feats {
                match feat {
                    Features::VIRTIO_NET_F_CSUM => continue,
                    Features::VIRTIO_NET_F_GUEST_CSUM => continue,
                    Features::VIRTIO_NET_F_CTRL_GUEST_OFFLOADS => continue,
                    Features::VIRTIO_NET_F_MTU => continue,
                    Features::VIRTIO_NET_F_MAC => continue,
                    Features::VIRTIO_NET_F_GUEST_TSO4 => {
                        if feat_bits & Features::VIRTIO_NET_F_GUEST_CSUM != 0 {
                            continue;
                        } else {
                            return Err(VirtioNetError::FeatReqNotMet(FeatureSet(feat_bits)))
                        }
                    },
                    Features::VIRTIO_NET_F_GUEST_TSO6 => {
                        if feat_bits & Features::VIRTIO_NET_F_GUEST_CSUM != 0 {
                            continue;
                        } else {
                            return Err(VirtioNetError::FeatReqNotMet(FeatureSet(feat_bits)))
                        }
                    },
                    Features::VIRTIO_NET_F_GUEST_ECN => {
                        if feat_bits & (Features::VIRTIO_NET_F_GUEST_TSO4 | Features::VIRTIO_NET_F_GUEST_TSO6)  != 0{
                            continue;
                        } else {
                            return Err(VirtioNetError::FeatReqNotMet(FeatureSet(feat_bits)))
                        }
                    },
                    Features::VIRTIO_NET_F_GUEST_UFO => {
                        if feat_bits & Features::VIRTIO_NET_F_GUEST_CSUM != 0 {
                            continue;
                        } else {
                            return Err(VirtioNetError::FeatReqNotMet(FeatureSet(feat_bits)))
                        }
                    },
                    Features::VIRTIO_NET_F_HOST_TSO4 => {
                        if feat_bits & Features::VIRTIO_NET_F_CSUM != 0 { 
                            continue;
                        } else {
                            return Err(VirtioNetError::FeatReqNotMet(FeatureSet(feat_bits)))
                        }
                    },
                    Features::VIRTIO_NET_F_HOST_TSO6 => {
                        if feat_bits & Features::VIRTIO_NET_F_CSUM != 0 {
                            continue;
                        } else {
                            return Err(VirtioNetError::FeatReqNotMet(FeatureSet(feat_bits)))
                        }
                    },
                    Features::VIRTIO_NET_F_HOST_ECN => {
                        if feat_bits & (Features::VIRTIO_NET_F_HOST_TSO4 | Features::VIRTIO_NET_F_HOST_TSO6) != 0 {
                            continue;
                        } else {
                            return Err(VirtioNetError::FeatReqNotMet(FeatureSet(feat_bits)))
                        }
                    },
                    Features::VIRTIO_NET_F_HOST_UFO => {
                        if feat_bits & Features::VIRTIO_NET_F_CSUM != 0 {
                            continue;
                        } else {
                            return Err(VirtioNetError::FeatReqNotMet(FeatureSet(feat_bits)))
                        }
                    },
                    Features::VIRTIO_NET_F_MRG_RXBUF => continue,
                    Features::VIRTIO_NET_F_STATUS => continue,
                    Features::VIRTIO_NET_F_CTRL_VQ => continue,
                    Features::VIRTIO_NET_F_CTRL_RX => {
                        if feat_bits & Features::VIRTIO_NET_F_CTRL_VQ != 0 {
                            continue;
                        } else {
                            return Err(VirtioNetError::FeatReqNotMet(FeatureSet(feat_bits)))
                        }
                    },
                    Features::VIRTIO_NET_F_CTRL_VLAN => {
                        if feat_bits & Features::VIRTIO_NET_F_CTRL_VQ != 0 {
                            continue;
                        } else {
                            return Err(VirtioNetError::FeatReqNotMet(FeatureSet(feat_bits)))
                        }
                    },
                    Features::VIRTIO_NET_F_GUEST_ANNOUNCE => {
                        if feat_bits & Features::VIRTIO_NET_F_CTRL_VQ != 0 {
                            continue;
                        } else {
                            return Err(VirtioNetError::FeatReqNotMet(FeatureSet(feat_bits)))
                        }
                    },
                    Features::VIRTIO_NET_F_MQ => {
                        if feat_bits & Features::VIRTIO_NET_F_CTRL_VQ != 0 {
                            continue;
                        } else {
                            return Err(VirtioNetError::FeatReqNotMet(FeatureSet(feat_bits)))
                        }
                    },
                    Features::VIRTIO_NET_F_CTRL_MAC_ADDR => {
                        if feat_bits & Features::VIRTIO_NET_F_CTRL_VQ != 0 {
                            continue;
                        } else {
                            return Err(VirtioNetError::FeatReqNotMet(FeatureSet(feat_bits)))
                        }
                    },
                    Features::VIRTIO_NET_F_GUEST_HDRLEN => continue,
                    Features::VIRTIO_NET_F_RSC_EXT => {
                        if feat_bits & (Features::VIRTIO_NET_F_HOST_TSO4 | Features::VIRTIO_NET_F_HOST_TSO6) != 0 {
                            continue;
                        } else {
                            return Err(VirtioNetError::FeatReqNotMet(FeatureSet(feat_bits)))
                        }
                    },
                    Features::VIRTIO_NET_F_STANDBY => continue,
                    Features::VIRTIO_F_RING_INDIRECT_DESC => continue,
                    Features::VIRTIO_F_RING_EVENT_IDX => continue,
                    Features::VIRTIO_F_VERSION_1 => continue,
                    Features::VIRTIO_F_ACCESS_PLATFORM => continue,
                    Features::VIRTIO_F_RING_PACKED => continue,
                    Features::VIRTIO_F_IN_ORDER => continue, 
                    Features::VIRTIO_F_ORDER_PLATFORM => continue,
                    Features::VIRTIO_F_SR_IOV => continue,
                    Features::VIRTIO_F_NOTIFICATION_DATA => continue, 
                } 
            }

            Ok(())
        }

        /// Checks if a given feature is set.
        pub fn is_feature(self, feat: Features) -> bool {
            self.0 & feat != 0
        }

        /// Sets features contained in feats to true.
        ///
        /// WARN: Features should be checked before using this function via the 
        /// `FeatureSet::check_features(feats: Vec<Features>) -> Result<(), VirtioNetError>` function.
        pub fn set_features(&mut self, feats: &Vec<Features>) {
            for feat in feats { 
                self.0 | *feat;
            }
        }

        /// Returns a new instance of (FeatureSet)[FeatureSet] with all features
        /// initalized to false. 
        pub fn new(val: u64) -> Self {
            FeatureSet(val)
        }
    }
}

/// Error module of virtios network driver. Containing the (VirtioNetError)[VirtioNetError]
/// enum.
pub mod error {
    use super::constants::FeatureSet;
    /// Network drivers error enum.
    #[derive(Debug, Copy, Clone)]
    pub enum VirtioNetError {
        General,
        NoDevCfg(u16),
        NoComCfg(u16),
        NoIsrCfg(u16),
        NoNotifCfg(u16),
        FailFeatureNeg(u16),
        /// Set of features does not adhere to the requirements of features 
        /// indicated by the specification
        FeatReqNotMet(FeatureSet),
        /// The first u64 contains the feature bits wanted by the driver.
        /// but which are incompatible with the device feature set, second u64.
        IncompFeatsSet(FeatureSet, FeatureSet)
    }
}
