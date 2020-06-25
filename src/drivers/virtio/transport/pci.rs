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
use arch::x86_64::kernel::pci::error::PciError;
use alloc::vec::Vec;
use core::result::Result;

use drivers::error::DriverError;
use drivers::virtio::error::VirtioError;
use drivers::virtio::types::{Le16, Le32, Le64};
use drivers::virtio::env::MemAddr;
use drivers::virtio::virtqueue::Virtq;
use drivers::net::virtio_net::VirtioNetDriver;

/// Virtio device ID's 
/// See Virtio specification v1.1. - 5 
///                      and v1.1. - 4.1.2.1
#[allow(dead_code, non_camel_case_types)]
#[repr(u16)]
pub enum DevId {
    INVALID = 0x0000,
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
impl From<u16> for DevId {
    fn from(id: u16) -> Self {
        match id {
            0x0fff => DevId::VIRTIO_TRANS_DEV_ID_NET,
            0x1001 => DevId::VIRTIO_TRANS_DEV_ID_BLK,
            0x1002 => DevId::VIRTIO_TRANS_DEV_ID_MEM_BALL,
            0x1003 => DevId::VIRTIO_TRANS_DEV_ID_CONS,
            0x1004 => DevId::VIRTIO_TRANS_DEV_ID_SCSI,
            0x1005 => DevId::VIRTIO_TRANS_DEV_ID_ENTROPY,
            0x1009 => DevId::VIRTIO_TRANS_DEV_ID_9P,
            0x1041 => DevId::VIRTIO_DEV_ID_NET,
            0x105A => DevId::VIRTIO_DEV_ID_FS,
            _ => DevId::INVALID,
        }
    }
}

/// Virtio's cfg_type constants; indicating type of structure in capabilities list
/// See Virtio specification v1.1 - 4.1.4
#[allow(dead_code, non_camel_case_types)]
#[repr(u8)]
pub enum CfgType {
    RESERVED = 0,
    VIRTIO_PCI_CAP_COMMON_CFG = 1,
    VIRTIO_PCI_CAP_NOTIFY_CFG = 2,
    VIRTIO_PCI_CAP_ISR_CFG = 3, 
    VIRTIO_PCI_CAP_DEVICE_CFG = 4,
    VIRTIO_PCI_CAP_PCI_CFG = 5, 
    VIRTIO_PCI_CAP_SHARED_MEMORY_CFG = 8,
} 

impl From<u8> for CfgType {
    fn from(val: u8) -> Self {
        match val {
            1 => CfgType::VIRTIO_PCI_CAP_COMMON_CFG,
            2 => CfgType::VIRTIO_PCI_CAP_NOTIFY_CFG,
            3 => CfgType::VIRTIO_PCI_CAP_ISR_CFG,
            4 => CfgType::VIRTIO_PCI_CAP_DEVICE_CFG,
            5 => CfgType::VIRTIO_PCI_CAP_PCI_CFG,
            8 => CfgType::VIRTIO_PCI_CAP_SHARED_MEMORY_CFG,
            _ => CfgType::RESERVED,
        }
    }
}
    

/// Virtio's PCI capabilites structure.
/// See Virtio specification v.1.1 - 4.1.4
///
/// Indicating: Where the capability field is mapped in memory and 
/// Which id (sometimes also indicates priority for multiple 
/// capabilites of same type) it holds.
///
/// This structure does NOT represent the structure in the standard,
/// as it is not directly mapped into address space from PCI device 
/// configuration space.
/// Therefore the struct only contains necessary information to map
/// corresponding [CfgType](enums.CfgType.html) into address space.
pub struct PciCap {
    cfg_type: CfgType,
    bar: u8,
    id: u8,
    offset: Le32,
    length: Le32,
    // TODO: PROVIDE LOCATION AND SO ON AS A SIMPLE MEMORY ADDRESS A DRIVER CAN SIMPLY MAP
    //       THE WHOLE MAPPING (PCI_KERNEL::PARSE_BARS UND PCI_KERNEL::MAP_VIRTIOCAP) SHOULD GO ON IN FUNCTION read_pci_caps()!
}

impl PciCap {
    fn get_type(&self) -> &CfgType{
        &self.cfg_type
    }

    fn get_id(&self) -> u8 {
        self.id
    }
}

/// Virtio's PCI capabilites structure for 64 bit capabilites.
/// See Virtio specification v.1.1 - 4.1.4
///
/// Only used for capabilites that require offsets or lengths 
/// larger than 4GB.
#[repr(C)]
struct PciCap64 {
    pci_cap: PciCap,
    offset_high: u32,
    length_high: u32,
} 

/// Universal Caplist Collections holds all universal capability structures for 
/// a given Virtio PCI device.
///
/// As Virtio's PCI devices are allowed to present multiple capability
/// structures of the same [CfgType](enums.cfgtype.html), the structure 
/// provides a driver with all capabilites, sorted in descending priority,
/// allowing the driver to choose.
/// The structure contains a special dev_cfg_list field, a vector holding 
/// [PciCap](structs.pcicap.html) objects, to allow the driver to map its
/// device specific configurations independently.
pub struct UniCapsColl {
    com_cfg_list: Vec<ComCfg>,
    notif_cfg_list: Vec<NotifCfg>,
    isr_stat_list: Vec<IsrStatus>,
    pci_cfg_acc_list: Vec<PciCfg>,
    sh_mem_cfg_list: Vec<ShMemCfg>,
    dev_cfg_list: Vec<PciCap>
}

impl UniCapsColl {
    /// Returns an Caps with empty lists.
    pub fn new() -> Self {
        UniCapsColl {
            com_cfg_list: Vec::new(),
            notif_cfg_list: Vec::new(),
            isr_stat_list: Vec::new(),
            pci_cfg_acc_list: Vec::new(),
            sh_mem_cfg_list: Vec::new(),
            dev_cfg_list: Vec::new(),
        }
    }
//
// TODO !!!!!!!!!!!!!!!!
//
// CHANGE ALL ADD FUNCTIONS TO TAKE AN NOT_RAW OBJECT AND WRAP ALL INTERACTION WITH THE RAW OBJECTS INTO NON_RAW OBJECTS TO PROVIDE SAVE INTERACTION
    fn add_cfg_common(&mut self, com_raw: ComCfgRaw, rank: u8) {
        self.com_cfg_list.push(ComCfg::new(com_raw, rank));
        // Resort array
        // 
        // This should not be to expensive, as "rational" devices will hold an
        // acceptibal amount of configuration structures.
        self.com_cfg_list.sort_by(|a, b| a.rank.cmp(&b.rank));
    }

    fn add_cfg_notif(&mut self, notif_raw: NotifCfgRaw, rank: u8) {
        self.notif_cfg_list.push(NotifCfg::new(notif_raw, rank));
        // Resort array
        // 
        // This should not be to expensive, as "rational" devices will hold an
        // acceptibal amount of configuration structures.
        self.notif_cfg_list.sort_by(|a, b| a.rank.cmp(&b.rank));
    }

    fn add_cfg_isr(&mut self, isr_raw: IsrStatusRaw, rank: u8) {
        self.isr_stat_list.push(IsrStatus::new(isr_raw, rank));
        // Resort array
        // 
        // This should not be to expensive, as "rational" devices will hold an
        // acceptibal amount of configuration structures.
        self.isr_stat_list.sort_by(|a, b| a.rank.cmp(&b.rank));
    }

    fn add_cfg_pci(&mut self, pci_raw: PciCfgRaw, rank: u8) {
        self.pci_cfg_acc_list.push(PciCfg::new(pci_raw, rank));
        // Resort array
        // 
        // This should not be to expensive, as "rational" devices will hold an
        // acceptibal amount of configuration structures.
        self.pci_cfg_acc_list.sort_by(|a, b| a.rank.cmp(&b.rank));
    }

    fn add_cfg_sh_mem(&mut self, sh_mem_raw: ShMemCfgRaw, id: u8) {
        self.sh_mem_cfg_list.push(ShMemCfg::new(sh_mem_raw, id));
        // Resort array
        // 
        // This should not be to expensive, as "rational" devices will hold an
        // acceptibal amount of configuration structures.
        self.sh_mem_cfg_list.sort_by(|a, b| a.id.cmp(&b.id));
    }

    fn add_cfg_dev(&mut self, pci_cap: PciCap) {
        self.dev_cfg_list.push(pci_cap);
        // Resort array
        // 
        // This should not be to expensive, as "rational" devices will hold an
        // acceptibal amount of configuration structures.
        self.dev_cfg_list.sort_by(|a, b| a.id.cmp(&b.id));
    }
}

/// Wraps a [ComCfgRaw](structs.comcfgraw.html) in order to preserve
/// the original structure.
///
/// Provides a safe API for Raw structure and allows interaction with the device via 
/// the structure. 
pub struct ComCfg {
    com_cfg: ComCfgRaw,
    pub rank: u8,
}

impl ComCfg {
    fn new(raw: ComCfgRaw, rank: u8) -> Self {
        ComCfg{
            com_cfg: raw,
            rank,
        }
    }

    pub fn write_vq_mem_areas(&self) {
        unimplemented!();
    }

    fn set_vq_desc_area(raw_cfg: &ComCfgRaw, desc_addr: MemAddr, vq_id: Le16,) {
        unimplemented!();
    }

    fn set_vq_dev_area(raw_cfg: &ComCfgRaw, dev_ddr: MemAddr, vq_id: Le16) {
        unimplemented!();
    }

    fn set_vq_driv_area(raw_cfg: &ComCfgRaw, desc_addr: MemAddr, vq_id: Le16) {
        unimplemented!();
    }
}

/// Common configuration structure of Virtio PCI devices.
/// See Virtio specification v1.1 - 4.1.43
/// 
/// Fields read-write-rules in source code refer to driver rights.
#[repr(C)]
struct ComCfgRaw {
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

// Common configuration raw does NOT provide a PUBLIC
// interface. 
impl ComCfgRaw {
    fn map(cap: &PciCap) -> Self {
        unimplemented!();
    }

    fn set_queue_size(&mut self, size: Le16, packed: bool) {
    // TODO:
    // Should set queue size and forbid to set queue size equal to zer0 if packed feature
    // has been negotiated.
    }

    fn read_dev_feat(&self) -> Le32 {
        self.device_feature
    }

    fn read_num_vq(&self) -> Le16 {
        self.num_queues
    }

    fn read_cfg_gen(&self) -> u8 {
        self.config_generation
    }

    fn read_notif(&self) -> Le16 {
        self.queue_notify_off
    }
}

/// Wraps a [NotifCfgRaw](structs.notifcfgraw.html) in order to preserve
/// the original structure and allow interaction with the device via 
/// the structure.
///
/// Provides a safe API for Raw structure and allows interaction with the device via 
/// the structure. 
pub struct NotifCfg {
    notif_cfg: NotifCfgRaw,
    pub rank: u8,
}

impl NotifCfg {
    fn new(raw: NotifCfgRaw, rank: u8) -> Self {
        NotifCfg {
            notif_cfg: raw,
            rank,
        }
    }
}

/// Notifcation structure of Virtio PCI devices.
/// See Virtio specification v1.1 - 4.1.4.4
///
// TODO:
// The notification struct is placed immediately after the 
// [PciCap](structs.PciCap.html) in memory.
struct NotifCfgRaw {
    notify_off_multiplier: Le32,  // Multiplier for queue_notify_off
}

impl NotifCfgRaw {
    pub fn map(cap: &PciCap) -> Self {
        unimplemented!();
    }
}

/// Wraps a [IsrStatusRaw](structs.isrstatusraw.html) in order to preserve
/// the original structure and allow interaction with the device via 
/// the structure.
///
/// Provides a safe API for Raw structure and allows interaction with the device via 
/// the structure. 
pub struct IsrStatus {
    isr_stat: IsrStatusRaw,
    pub rank: u8
}

impl IsrStatus {
    fn new(raw: IsrStatusRaw, rank: u8) -> Self {
        IsrStatus {
            isr_stat: raw,
            rank,
        }
    }
}

/// ISR status structure of Virtio PCI devices.
/// See Virtio specification v1.1. - 4.1.4.5
///
/// Contains a single byte, containing the interrupt numnbers used
/// for handling interrupts. 
/// The 8-bit field is read as an bitmap and allows to distinguish between
/// interrupts triggered by changes in the configuration and interrupts
/// triggered by events of a virtqueue.
struct IsrStatusRaw {
    flags: u8,
}

impl IsrStatusRaw {
    fn map(cap: &PciCap) -> Self {
        unimplemented!();
    }

    // returns true if second bit, from left is 1.
    // read MUST reset flag
    fn cfg_event() -> bool {
        unimplemented!();
    }

    // returns trie if first bit, from left is 1.
    // read MUST reset flag
    fn vqueue_event() -> bool {
        unimplemented!();
    }
}

/// Wraps a [PciCfgRaw](structs.pcicfgraw.html) in order to preserve
/// the original structure and allow interaction with the device via 
/// the structure.
///
/// Provides a safe API for Raw structure and allows interaction with the device via 
/// the structure. 
pub struct PciCfg {
    pci_cfg: PciCfgRaw,
    pub rank: u8,
}

impl PciCfg {
    fn new(raw: PciCfgRaw, rank: u8) -> Self {
        PciCfg {
            pci_cfg: raw, 
            rank, 
        }
    }
}

/// PCI configuration access structure of Virtio PCI devices.
/// See Virtio specification v1.1. - 4.1.4.8
///
/// ONLY an alternative access method to the common configuration, notification,
/// ISR and device-specific configuration regions/structures.
struct PciCfgRaw {
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

impl PciCfgRaw {
    fn map(cap: &PciCap) -> Self {
        unimplemented!();
    }
}

/// Wraps a [ShMemCfgRaw](structs.shmemcfgraw.html) in order to preserve
/// the original structure and allow interaction with the device via 
/// the structure.
///
/// Provides a safe API for Raw structure and allows interaction with the device via 
/// the structure. 
pub struct ShMemCfg {
    sh_mem_raw: ShMemCfgRaw,
    // Shared memory regions are identified via an ID
    // See Virtio specification v1.1. - 4.1.4.7
    pub id: u8,
}

impl ShMemCfg {
    fn new(raw: ShMemCfgRaw, id: u8) -> Self {
        ShMemCfg {
            sh_mem_raw: raw,
            id,
        }
    }
}
/// Shared memory configuration structure of Virtio PCI devices.
/// See Virtio specification v1.1. - 4.1.4.7
///
/// Each shared memory region is defined via a single shared
/// memory structure. Each region is identified by an id indicated
/// via the capability.id field of 
struct ShMemCfgRaw {
    // TODO:
    // The region defined by the combination of the cap.offset, cap.offset_hi, and cap.length, cap.length_hi fields MUST be contained within the declared bar.
    // The cap.id MUST be unique for any one device instance.
}

impl ShMemCfgRaw {
    fn map(cap: &PciCap) -> Self {
        unimplemented!();
    }
} 

/// Reads all PCI capabilities, starting at the capabilites list pointer from the 
/// PCI device. 
///
/// Returns ONLY Virtio specific capabilites, which allow to locate the actual capability 
/// structures inside the memory areas, indicate by the BaseAddressRegisters (BARS).
fn read_pci_caps(adapter: &PciAdapter) -> Vec<PciCap> {
    unimplemented!();
    // PARSE BARS FIELDS OF PCI CONFIGURATION SPACE, NO MAPPING AS THIS IS NOT NEEDED; JUST THE START ADDRESS AND THE LENGTH OF THE ADDRESS
    //
    // LOOP THROUGH CAPABILTY LIST STARTING AT CAPABILITES POINTER AND CREATE A PciCap STRCUTRUE FOR EACH CAPABILITY; CONTAINING THE ACTUAL PHYSISCAL MEMORY
    // ADDRESS FOR THE MAP FUNCTIONS TO MAP TO!
}

pub fn map_caps(adapter: &PciAdapter) -> Result<UniCapsColl, PciError> {
    // Get list of PciCaps pointing to capabilities
    let pci_cap_list = read_pci_caps(adapter);

    let mut caps = UniCapsColl::new();
    // Map Caps in virtual memory
    for pci_cap in pci_cap_list {
        match pci_cap.get_type() {
            CfgType::VIRTIO_PCI_CAP_COMMON_CFG =>  caps.add_cfg_common(ComCfgRaw::map(&pci_cap), pci_cap.get_id()),
            CfgType::VIRTIO_PCI_CAP_NOTIFY_CFG => caps.add_cfg_notif(NotifCfgRaw::map(&pci_cap), pci_cap.get_id()),
            CfgType::VIRTIO_PCI_CAP_ISR_CFG => caps.add_cfg_isr(IsrStatusRaw::map(&pci_cap), pci_cap.get_id()),
            CfgType::VIRTIO_PCI_CAP_PCI_CFG => caps.add_cfg_pci(PciCfgRaw::map(&pci_cap), pci_cap.get_id()),
            CfgType::VIRTIO_PCI_CAP_SHARED_MEMORY_CFG => caps.add_cfg_sh_mem(ShMemCfgRaw::map(&pci_cap), pci_cap.get_id()),
            CfgType::VIRTIO_PCI_CAP_DEVICE_CFG => caps.add_cfg_dev(pci_cap),
            // PCI's configuration space is allowed to hold other structures, which are not virtio specific and are therefore ignored
            // in the following
            _ => continue,
        }
    }

    Err(PciError::General(adapter))
}

/// Checks existing drivers for support of given device. Upon match, provides
/// driver with a [Caplist](struct.Caplist.html) struct, holding the structures of the capabilities
/// list of the given device.
pub fn init_device(adapter: &PciAdapter) -> Result<PciDriver, DriverError> {
    match DevId::from(adapter.device_id) {
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
		DevId::VIRTIO_DEV_ID_NET => {
            match VirtioNetDriver::init(adapter) {
                Ok(virt_net_drv) => {
                    info!(
                        "Virtio network driver initalized with Virtio network device."
                    );
                    return Ok(PciDriver::VirtioNetNew(virt_net_drv))
                },
                Err(virtio_error) => {
                    warn!(
                        "Virtio networkd driver could not be initalized with device: {:x}",
                        adapter.device_id
                    );
                    return Err(DriverError::InitVirtioDevFail(virtio_error))
                },
            };
        },
		DevId::VIRTIO_DEV_ID_FS => {
		    // TODO: Proper error handling in driver creation fail
            //virtio_fs::create_virtiofs_driver(adapter).unwrap()

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
}

pub fn read_cfg(bus: u8, device: u8, register: u32) -> MemAddr {
    MemAddr::from(pci::read_config(bus, device, register))
}
