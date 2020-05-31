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
use core::result::Result;
use alloc::boxed::Box;

use drivers::error::DriverError;
use drivers::virtio::error::VirtioError;
use drivers::virtio::transport::pci::{Caplist, DevCfg};
use drivers::virtio::transport::pci;
use drivers::virtio::driver::VirtioDriver;
use drivers::virtio::types::{Le16};

/// Virtio's network device feature bits
/// See Virtio specficiation v1.1. - 5.1.3
#[repr(u32)]
pub enum NetFeature {
    VIRTIO_NET_F_CSUM= 0,
    VIRTIO_NET_F_GUEST_CSUM = 1,
    VIRTIO_NET_F_CTRL_GUEST_OFFLOADS = 2,
    VIRTIO_NET_F_MTU = 3,
    VIRTIO_NET_F_MAC = 5,
    VIRTIO_NET_F_GUEST_TSO4 = 7,
    VIRTIO_NET_F_GUEST_TSO6 = 8,
    VIRTIO_NET_F_GUEST_ECN = 9,
    VIRTIO_NET_F_GUEST_UFO = 10,
    VIRTIO_NET_F_HOST_TSO4 = 11,
    VIRTIO_NET_F_HOST_TSO6 = 12,
    VIRTIO_NET_F_HOST_ECN = 13,
    VIRTIO_NET_F_HOST_UFO = 14,
    VIRTIO_NET_F_MRG_RXBUF = 15,
    VIRTIO_NET_F_STATUS = 16,
    VIRTIO_NET_F_CTRL_VQ = 17,
    VIRTIO_NET_F_CTRL_RX = 18,
    VIRTIO_NET_F_CTRL_VLAN = 19,
    VIRTIO_NET_F_CTRL_RX_EXTRA = 20,
    VIRTIO_NET_F_GUEST_ANNOUNCE = 21,
    VIRTIO_NET_F_MQ = 22,
    VIRTIO_NET_F_CTRL_MAC_ADDR = 23,
    VIRTIO_NET_F_GSO = 6,
}
/// Virtio's network device configuration structure. 
/// See specification v1.1. - 5.1.4
///
#[repr(C)]
struct NetDevCfg {
	mac: [u8; 6],
	status: Le16,
	max_virtqueue_pairs: Le16,
	mtu: Le16,
}

impl DevCfg for NetDevCfg {
}

impl NetDevCfg {
    /// Instatiates a zero initalized virtio network device config.
    /// This "empty" struct will later be dereferenced and mapped to a different position.
    pub fn new() -> Self {
        NetDevCfg {
            mac: [0; 6],
            status: Le16(0),
            max_virtqueue_pairs: Le16(0),
            mtu: Le16(0),
        }
    }
}

pub struct VirtioNetDriver {

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

impl VirtioNetDriver {
    pub fn new() -> Self {
        VirtioNetDriver {

        }
    }

    /// Initializes virtio network device by mapping configuration layout to 
    /// respective structs and creating a new driver instance for the device.
    pub fn init_device(adapter: &PciAdapter) -> Result<VirtioNetDriver, DriverError> {
        let dev_cfg = Box::new(NetDevCfg::new());

        match pci::map_caplist(adapter, dev_cfg) {
            Ok(caplist) => {
                //
                Ok(VirtioNetDriver::new())
            },
            Err(VirtioError) => {
                Err(DriverError::InitVirtioDevFail(VirtioError))        
            } 
        }
        
    }
}

