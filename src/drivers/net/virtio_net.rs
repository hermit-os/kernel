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
use core::result::Result;
use alloc::vec::Vec;
use core::mem;
use core::ops::Deref;

use drivers::virtio::env::memory::{MemLen, MemOff};
use drivers::virtio::transport::pci::{UniCapsColl, ComCfg, ShMemCfg, NotifCfg, IsrStatus, PciCfgAlt, PciCap};
use drivers::virtio::transport::pci;
use drivers::virtio::driver::VirtioDriver;
use drivers::virtio::error::VirtioError;
use drivers::virtio::virtqueue::Virtq;
use drivers::virtio::virtqueue::packed::PackedVq;
use drivers::virtio::virtqueue::split::SplitVq;

use self::error::VirtioNetError;
use self::constants::{Features, Status, FeatureSet, MAX_NUM_VQ};



/// A wrapper struct for the raw configuration structure. 
/// Handling the right access to fields, as some are read-only
/// for the driver.
///
/// 
pub struct NetDevCfg {
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

/// Virtio network driver struct. 
///
/// Struct allows to control devices virtqueues as also
/// the device itself.
pub struct VirtioNetDriver {
    dev_cfg: NetDevCfg,
    com_cfg: ComCfg,
    isr_stat: IsrStatus,
    notif_cfg: NotifCfg,

    ctrl_vq: Option<Virtq>,
    recv_vqs: Option<Vec<Virtq>>, 
    send_vqs: Option<Vec<Virtq>>,
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
            features: FeatureSet::new(),
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

            ctrl_vq: None,
            recv_vqs: None,
            send_vqs: None,
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

        // Define wanted feature set
        let wanted_feats = vec![ Features::VIRTIO_NET_F_GUEST_CSUM,
            Features::VIRTIO_NET_F_MAC, 
            Features::VIRTIO_NET_F_STATUS,
            Features::VIRTIO_NET_F_GUEST_TSO4,
            Features::VIRTIO_NET_F_GUEST_TSO6,
        ];

        // Negotiate features with device. If feature set is 
        match self.negotiate_features(&wanted_feats) {
            Ok(_) => info!("Driver found a subset of features for virtio device {:x}.", self.dev_cfg.dev_id),
            Err(vnet_err) => {
                // A new feature negotiation with a reduced feature set up to the point of minimal feat_set
                // which should be defined somewhere goes HERE.
                error!("Wanted set of features is NOT supported by device. Set: {:?}", wanted_feats);
                return Err(vnet_err)
            },
        }
        
        // Indicates the device, that the current feature set is final for the driver
        // and will not be changed.
        self.com_cfg.features_ok();

        // Checks if the device has accepted final set. This finishes feature negotiation.
        if self.com_cfg.check_features() {
            info!("Features have been negotiated between network device {:x} and driver.", self.dev_cfg.dev_id);
            // Set feature set in device config fur future use.
            self.dev_cfg.features.set_features(&wanted_feats);
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
        let mut drv_feats: u64 = 0;
        
        for feat in wanted_feats.iter() {
            drv_feats |= *feat;
        }

        let dev_feats = self.com_cfg.dev_features();
        
        // Checks if the selected feature set is compatible with requirements for 
        // features according to Virtio spec. v1.1 - 5.1.3.1.
        match FeatureSet::check_features(&wanted_feats) {
            Ok(_) => info!("Feature set wanted by network driver, matches virtio netword devices capabiliites."),
            Err(vnet_err) => return Err(VirtioNetError::IncompFeatsSet(drv_feats, dev_feats)),
        }

        if dev_feats & drv_feats == drv_feats {
            // If device supports subset of features write feature set to common config
            self.com_cfg.set_drv_features(drv_feats);
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

        if self.dev_cfg.features.is_feature(Features::VIRTIO_NET_F_CTRL_VQ) {
            unimplemented!()
        }




        // PLACEHOLDER FOR COMPILER
        Err(VirtioNetError::General)
    }

    /// Initalize virtqueues via the queue interface and populates receiving queues
    fn virtqueue_init(&mut self) -> Result<(), VirtioNetError> {
        let num_vq: u16;
        if self.dev_cfg.features.is_feature(Features::VIRTIO_NET_F_MQ) {
            if self.dev_cfg.raw.max_virtqueue_pairs <= MAX_NUM_VQ {
                num_vq = MAX_NUM_VQ;
            } else {
                num_vq = self.dev_cfg.raw.max_virtqueue_pairs;
            }
        } else {
            // Minimal number of virtqueues defined in the standard v1.1. - 5.1.5 Step 1
            num_vq = 2;
        }

        let recv_vqs: Vec<Virtq> = Vec::with_capacity(num_vq as usize);
        let send_vqs: Vec<Virtq> = Vec::with_capacity(num_vq as usize);

        for i in 0..num_vq {
            
        }


        // PLACEHOLDER FOR COMPILER
        Err(VirtioNetError::General)
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
        self.dev_cfg.raw.status
    }
}

mod constants {
    use core::ops::{BitOr, BitOrAssign, BitAnd, BitAndAssign};
    use super::error::VirtioNetError; 
    use alloc::vec::Vec;

    // Configuration constants
    pub const MAX_NUM_VQ:u16 = 2;
    
    /// Enum contains virtio's network device features and general features of Virtio.
    ///
    /// See Virtio specification v1.1. - 5.1.3
    /// 
    /// See Virtio specification v1.1. - 6
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

        // 5.1.3.1 Feature bit requirements
        // Some networking feature bits require other networking feature bits (see 2.2.1): VIRTIO_NET_F_GUEST_TSO4 Requires VIRTIO_NET_F_GUEST_CSUM.
        // VIRTIO_NET_F_GUEST_TSO6 Requires VIRTIO_NET_F_GUEST_CSUM.
        // VIRTIO_NET_F_GUEST_ECN RequiresVIRTIO_NET_F_GUEST_TSO4orVIRTIO_NET_F_GUEST_TSO6. VIRTIO_NET_F_GUEST_UFO Requires VIRTIO_NET_F_GUEST_CSUM.
        // VIRTIO_NET_F_HOST_TSO4 Requires VIRTIO_NET_F_CSUM.
        // VIRTIO_NET_F_HOST_TSO6 Requires VIRTIO_NET_F_CSUM.
        // VIRTIO_NET_F_HOST_ECN Requires VIRTIO_NET_F_HOST_TSO4 or VIRTIO_NET_F_HOST_TSO6. VIRTIO_NET_F_HOST_UFO Requires VIRTIO_NET_F_CSUM.
        // VIRTIO_NET_F_CTRL_RX Requires VIRTIO_NET_F_CTRL_VQ.
        // VIRTIO_NET_F_CTRL_VLAN Requires VIRTIO_NET_F_CTRL_VQ. VIRTIO_NET_F_GUEST_ANNOUNCE Requires VIRTIO_NET_F_CTRL_VQ.
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
    #[derive(Debug, Copy, Clone)]
    pub struct FeatureSet(u64);

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
                            return Err(VirtioNetError::FeatReqNotMet(feat_bits))
                        }
                    },
                    Features::VIRTIO_NET_F_GUEST_TSO6 => {
                        if feat_bits & Features::VIRTIO_NET_F_GUEST_CSUM != 0 {
                            continue;
                        } else {
                            return Err(VirtioNetError::FeatReqNotMet(feat_bits))
                        }
                    },
                    Features::VIRTIO_NET_F_GUEST_ECN => {
                        if feat_bits & (Features::VIRTIO_NET_F_GUEST_TSO4 | Features::VIRTIO_NET_F_GUEST_TSO6)  != 0{
                            continue;
                        } else {
                            return Err(VirtioNetError::FeatReqNotMet(feat_bits))
                        }
                    },
                    Features::VIRTIO_NET_F_GUEST_UFO => {
                        if feat_bits & Features::VIRTIO_NET_F_GUEST_CSUM != 0 {
                            continue;
                        } else {
                            return Err(VirtioNetError::FeatReqNotMet(feat_bits))
                        }
                    },
                    Features::VIRTIO_NET_F_HOST_TSO4 => {
                        if feat_bits & Features::VIRTIO_NET_F_CSUM != 0 { 
                            continue;
                        } else {
                            return Err(VirtioNetError::FeatReqNotMet(feat_bits))
                        }
                    },
                    Features::VIRTIO_NET_F_HOST_TSO6 => {
                        if feat_bits & Features::VIRTIO_NET_F_CSUM != 0 {
                            continue;
                        } else {
                            return Err(VirtioNetError::FeatReqNotMet(feat_bits))
                        }
                    },
                    Features::VIRTIO_NET_F_HOST_ECN => {
                        if feat_bits & (Features::VIRTIO_NET_F_HOST_TSO4 | Features::VIRTIO_NET_F_HOST_TSO6) != 0 {
                            continue;
                        } else {
                            return Err(VirtioNetError::FeatReqNotMet(feat_bits))
                        }
                    },
                    Features::VIRTIO_NET_F_HOST_UFO => {
                        if feat_bits & Features::VIRTIO_NET_F_CSUM != 0 {
                            continue;
                        } else {
                            return Err(VirtioNetError::FeatReqNotMet(feat_bits))
                        }
                    },
                    Features::VIRTIO_NET_F_MRG_RXBUF => continue,
                    Features::VIRTIO_NET_F_STATUS => continue,
                    Features::VIRTIO_NET_F_CTRL_VQ => continue,
                    Features::VIRTIO_NET_F_CTRL_RX => {
                        if feat_bits & Features::VIRTIO_NET_F_CTRL_VQ != 0 {
                            continue;
                        } else {
                            return Err(VirtioNetError::FeatReqNotMet(feat_bits))
                        }
                    },
                    Features::VIRTIO_NET_F_CTRL_VLAN => {
                        if feat_bits & Features::VIRTIO_NET_F_CTRL_VQ != 0 {
                            continue;
                        } else {
                            return Err(VirtioNetError::FeatReqNotMet(feat_bits))
                        }
                    },
                    Features::VIRTIO_NET_F_GUEST_ANNOUNCE => {
                        if feat_bits & Features::VIRTIO_NET_F_CTRL_VQ != 0 {
                            continue;
                        } else {
                            return Err(VirtioNetError::FeatReqNotMet(feat_bits))
                        }
                    },
                    Features::VIRTIO_NET_F_MQ => {
                        if feat_bits & Features::VIRTIO_NET_F_CTRL_VQ != 0 {
                            continue;
                        } else {
                            return Err(VirtioNetError::FeatReqNotMet(feat_bits))
                        }
                    },
                    Features::VIRTIO_NET_F_CTRL_MAC_ADDR => {
                        if feat_bits & Features::VIRTIO_NET_F_CTRL_VQ != 0 {
                            continue;
                        } else {
                            return Err(VirtioNetError::FeatReqNotMet(feat_bits))
                        }
                    },
                    Features::VIRTIO_NET_F_GUEST_HDRLEN => continue,
                    Features::VIRTIO_NET_F_RSC_EXT => {
                        if feat_bits & Features::VIRTIO_NET_F_HOST_TSO4 & Features::VIRTIO_NET_F_HOST_TSO6 != 0 {
                            continue;
                        } else {
                            return Err(VirtioNetError::FeatReqNotMet(feat_bits))
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
        pub fn new() -> Self {
            FeatureSet(0)
        }
    }
}

/// Error module of virtios network driver. Containing the (VirtioNetError)[VirtioNetError]
/// enum.
pub mod error {
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
        FeatReqNotMet(u64),
        /// The first u64 contains the feature bits wanted by the driver.
        /// but which are incompatible with the device feature set, second u64.
        IncompFeatsSet(u64, u64)
    }
}
