// Copyright (c) 2020 Thomas Lambertz, RWTH Aachen University
//               2020 Frederik Schulz, RWTH Aachen Univerity
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

//! A module containing all virtio specific pci functionality
//! 
//! The module contains ...

use arch::x86_64::kernel::pci::{PciAdapter, PciDriver};
use arch::x86_64::kernel::pci;
use alloc::vec::Vec;
use core::option::Option;
use core::result::Result;
use alloc::boxed::Box;

use drivers::net::virtio_net;
use drivers::error::DriverError;
use drivers::virtio::error::VirtioError;
use drivers::virtio::types::{Le16, Le32, Le64};

/// Virtio device ID's 
/// See Virtio specification v1.1. - 5 
///                      and v1.1. - 4.1.2.1
#[repr(u16)]
pub enum DevId {
    VIRTIO_TRANS_DEV_ID_NET = 0x0fff,
    VIRTIO_TRANS_DEV_ID_BLK = 0x1001,
    VIRTIO_TRANS_DEV_ID_MEM_BALL = 0x1002,
    VIRTIO_TRANS_DEV_ID_CONS = 0x1003,
    VIRTIO_TRANS_DEV_ID_SCSI = 0x1004,
    VIRTIO_TRANS_DEV_ID_ENTROPY = 0x1005,
    VIRTIO_TRANS_DEV_ID_9P = 0x1009,
    VIRTIO_DEV_ID_NET = 0x1041,
    VIRTIO_DEV_ID_FS = 0x105A,
}

/// Creates a device id enum variant of the u16 value. 
///
/// The enum variant is wrapped by an Option, which is neccessary,
/// as not all u16 values are assigned an DevId variant. Panicking
/// is not an option, as input is created during runtime and from 
/// external sources (i.e. PCI devices).
impl DevId {
    fn from_u16(id: u16) -> Option<Self> {
        match id {
            0x0fff => Some(DevId::VIRTIO_TRANS_DEV_ID_NET),
            0x1001 => Some(DevId::VIRTIO_TRANS_DEV_ID_BLK),
            0x1002 => Some(DevId::VIRTIO_TRANS_DEV_ID_MEM_BALL),
            0x1003 => Some(DevId::VIRTIO_TRANS_DEV_ID_CONS),
            0x1004 => Some(DevId::VIRTIO_TRANS_DEV_ID_SCSI),
            0x1005 => Some(DevId::VIRTIO_TRANS_DEV_ID_ENTROPY),
            0x1009 => Some(DevId::VIRTIO_TRANS_DEV_ID_9P),
            0x1041 => Some(DevId::VIRTIO_DEV_ID_NET),
            0x105A => Some(DevId::VIRTIO_DEV_ID_FS),
            _ => None,
        }
    }
}

/// Virtio's cfg_type constants; indicating type of structure in capabilities list
/// See Virtio specification v1.1 - 4.1.4
#[repr(u8)]
pub enum CfgType {
    VIRTIO_PCI_CAP_COMMON_CFG = 1,
    VIRTIO_PCI_CAP_NOTIFY_CFG = 2,
    VIRTIO_PCI_CAP_ISR_CFG = 3, 
    VIRTIO_PCI_CAP_DEVICE_CFG = 4,
    VIRTIO_PCI_CAP_PCI_CFG = 5, 
    VIRTIO_PCI_CAP_SHARED_MEMORY_CFG = 8,
}

/// Virtio's PCI capabilites structure.
/// See Virtio specification v.1.1 - 4.1.4
///
/// Indicating: Where the capability field is mapped in memory and 
/// Which id (sometimes also indicates priority for multiple 
/// capabilites of same type) it holds.
#[repr(C)]
struct PciCap {
    cap_vndr: u8, 
    cap_next: u8,
    cap_len: u8,
    cfg_type: u8,
    bar: u8,
    id: u8,
    padding: [u8;2],
    offset: Le32,
    length: Le32,
}
/// Virtio's PCI capabilites structure for 64 bit capabilites.
/// See Virtio specification v.1.1 - 4.1.4
///
/// Only used for capabilites that require offsets or lengths 
/// larger than 4GB.
#[repr(C)]
struct PciCap64 {
    PciCap: PciCap,
    offset_high: u32,
    length_high: u32,
} 

/// Caplist holds all capability structures for 
/// a given Virtio PCI device.
pub struct Caplist {
    com_cfg_list: Vec<ComCfg>,
    notif_cfg_list: Vec<NotifCfg>,
    ist_stat_list: Vec<IsrStatus>,
    pci_cfg_acc_list: Vec<PciCfgAcc>,
    sh_mem_cfg_list: Vec<ShMemCfg>,
    dev_cfg_list: Vec<Box<dyn DevCfg>>,
}

/// Common configuration structure of Virtio PCI devices.
/// See Virtio specification v1.1 - 4.1.43
/// 
/// Fields read-write-rules in source code refer to driver rights.
#[repr(C)]
struct ComCfg {
    // About whole device
    device_feature_select: Le32, // read-write 
    device_feature: Le32, // read-only for driver
    driver_feature_select: Le32,  // read-write
    driver_feature: Le32,  // read-write
    config_msix_vector: Le16,  // read-write
    num_queues: Le16,  // read-only for driver
    device_status: u8, // read-write
    config_generation: u8,  // read-only for driver

    // About a specific virtqueue
    queue_select: Le16,  // read-write 
    queue_size: Le16,  // read-write
    queue_msix_vector: Le16, // read-write 
    queue_enable: Le16, // read-write
    queue_notify_off: Le16, // read-only for driver 
    queue_desc: Le64, // read-write
    queue_driver: Le64, // read-write 
    queue_device: Le64, // read-write
}

impl ComCfg {
    // TODO:
    // Should set queue size and forbid to set queue size equal to zer0 if packed feature
    // has been negotiated.
    pub fn set_queue_size(size: Le16, packed: bool) {}
}

/// Notifcation structure of Virtio PCI devices.
/// See Virtio specification v1.1 - 4.1.4.4
///
// TODO:
// The notification struct is placed immediately after the 
// [PciCap](structs.PciCap.html) in memory.
struct NotifCfg {
    notify_off_multiplier: Le32,  // Multiplier for queue_notify_off
}

/// ISR status structure of Virtio PCI devices.
/// See Virtio specification v1.1. - 4.1.4.5
///
/// Contains a single byte, containing the interrupt numnbers used
/// for handling interrupts. 
/// The 8-bit field is read as an bitmap and allows to distinguish between
/// interrupts triggered by changes in the configuration and interrupts
/// triggered by events of a virtqueue.
struct IsrStatus {
    flags: u8,
}

impl IsrStatus {
    // returns true if second bit, from left is 1.
    // read MUST reset flag
    pub fn cfg_event() -> bool {
        unimplemented!();
    }

    // returns trie if first bit, from left is 1.
    // read MUST reset flag
    pub fn vqueue_event() -> bool {
        unimplemented!();
    }
}

/// PCI configuration access structure of Virtio PCI devices.
/// See Virtio specification v1.1. - 4.1.4.8
///
/// ONLY an alternative access method to the common configuration, notification,
/// ISR and device-specific configuration regions/structures.
struct PciCfgAcc {
    pci_cfg_data: [u8;4], // Data for BAR access
    // TODO:
    // The fields cap.bar, cap.length, cap.offset and pci_cfg_data are read-write (RW) for the driver.
    // To access a device region, the driver writes into the capability structure (ie. within the PCI configuration
    // space) as follows:
    // • The driver sets the BAR to access by writing to cap.bar.
    // • The  driver sets the size of the access by writing 1, 2 or 4 to cap.length. 
    // • The driver sets the offset within the BAR by writing to cap.offset.
    // At that point, pci_cfg_data will provide a window of size cap.length into the given cap.bar at offset cap.offset.
}

/// Shared memory configuration structure of Virtio PCI devices.
/// See Virtio specification v1.1. - 4.1.4.7
///
/// Each shared memory region is defined via a single shared
/// memory structure. Each region is identified by an id indicated
/// via the capability.id field of 
struct ShMemCfg {
    // TODO:
    // The region defined by the combination of the cap.offset, cap.offset_hi, and cap.length, cap.length_hi fields MUST be contained within the declared bar.
    // The cap.id MUST be unique for any one device instance.
}

/// Trait to unify virtio device configs.
/// 
/// Virtio drivers must provide a struct containing the respectives
/// device specific configuration layout.
pub trait DevCfg {
    /// New_map takes a physical address and maps the given 
    /// device configuration struct to point to this position.
    /// The empty configuration struct is dereferenced.
    fn new_map(&self, ) {
        
    }
}

pub fn map_caplist(adapter: &PciAdapter, vdev_cfg: Box<dyn DevCfg>) -> Result<Caplist, VirtioError> {
    unimplemented!();
}

/// Checks existing drivers for support of given device. Upon match, provides
/// driver with a [Caplist](struct.Caplist.html) struct, holding the structures of the capabilities
/// list of the given device.
pub fn init_device(adapter: &PciAdapter) -> Result<PciDriver, DriverError> {
    match DevId::from_u16(adapter.device_id) {
        Some(dev_id) => {
            match dev_id {
                DevId::VIRTIO_TRANS_DEV_ID_NET | 
                DevId::VIRTIO_TRANS_DEV_ID_BLK | 
                DevId::VIRTIO_TRANS_DEV_ID_MEM_BALL |
                DevId::VIRTIO_TRANS_DEV_ID_CONS |
                DevId::VIRTIO_TRANS_DEV_ID_SCSI |
                DevId::VIRTIO_TRANS_DEV_ID_ENTROPY |
                DevId::VIRTIO_TRANS_DEV_ID_9P => {
			        warn!(
                         "Legacy/transitional Virtio device, with id: 0x{:x} is NOT supported, skipping!",
                        adapter.device_id
                    );

                    // Return Driver error inidacting device is not supported
			        Err(DriverError::InitVirtioDevFail(VirtioError::DevNotSupported(adapter.device_id)))
		        },
		        VIRTIO_DEV_ID_NET => {
                    virtio_net::VirtioNetDriver::init_device(adapter);
                    // PLACEHOLDER TO GET RIGHT RETURN
                    Err(DriverError::InitVirtioDevFail(VirtioError::DevNotSupported(adapter.device_id)))
                },
		        VIRTIO_DEV_ID_FS => {
			        // TODO: Proper error handling in driver creation fail
                    //virtio_fs::create_virtiofs_driver(adapter).unwrap();

                    // PLACEHOLDER TO GET RIGHT RETURN
 			        Err(DriverError::InitVirtioDevFail(VirtioError::DevNotSupported(adapter.device_id)))
		        },
		        _ => {
			        warn!(
                    "Virtio device with id: 0x{:x} is NOT supported, skipping!", 
                    adapter.device_id
                    );

                    // Return Driver error inidacting device is not supported
			        Err(DriverError::InitVirtioDevFail(VirtioError::DevNotSupported(adapter.device_id)))
		        },

            }
        },
        None => Err(DriverError::InitVirtioDevFail(VirtioError::DevNotSupported(adapter.device_id))),
    }
}
