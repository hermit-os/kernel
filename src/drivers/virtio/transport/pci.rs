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
use arch::x86_64::kernel::pci as kernel_pci;
use arch::x86_64::kernel::pci::error::PciError;
use alloc::vec::Vec;
use alloc::boxed::Box;
use core::result::Result;
use core::convert::TryInto;
use core::mem;

use drivers::error::DriverError;
use drivers::virtio::error::VirtioError;
use drivers::virtio::types::{Le16, Le32, Le64};
use drivers::virtio::env::memory::{VirtMemAddr, PhyMemAddr, Offset};
use drivers::virtio::virtqueue::Virtq;
use drivers::virtio::env;
use drivers::net::virtio_net::VirtioNetDriver;

/// Virtio device ID's 
/// See Virtio specification v1.1. - 5 
///                      and v1.1. - 4.1.2.1
///
// WARN: Upon changes in the set of the enum variants
// one MUST adjust the associated From<u16> 
// implementation, in order catch all cases correctly,
// as this function uses the catch-all "_" case!
#[allow(dead_code, non_camel_case_types)]
#[repr(u16)]
pub enum DevId {
    INVALID = 0x0,
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

impl From<DevId> for u16 {
    fn from(val: DevId) -> u16 {
        match val {
            DevId::VIRTIO_TRANS_DEV_ID_NET => 0x0fff,
            DevId::VIRTIO_TRANS_DEV_ID_BLK => 0x1001,
            DevId::VIRTIO_TRANS_DEV_ID_MEM_BALL => 0x1002,
            DevId::VIRTIO_TRANS_DEV_ID_CONS => 0x1003,
            DevId::VIRTIO_TRANS_DEV_ID_SCSI => 0x1004, 
            DevId::VIRTIO_TRANS_DEV_ID_ENTROPY => 0x1005, 
            DevId::VIRTIO_TRANS_DEV_ID_9P => 0x1009, 
            DevId::VIRTIO_DEV_ID_NET => 0x1041, 
            DevId::VIRTIO_DEV_ID_FS => 0x105A, 
            DevId::INVALID => 0x0,
        }
    }
}

impl From<u16> for DevId {
    fn from(val: u16) -> Self {
        match val {
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
//
// WARN: Upon changes in the set of the enum variants
// one MUST adjust the associated From<u8> 
// implementation, in order catch all cases correctly,
// as this function uses the catch-all "_" case! 
#[allow(dead_code, non_camel_case_types)]
#[repr(u8)]
pub enum CfgType {
    INVALID = 0,
    VIRTIO_PCI_CAP_COMMON_CFG = 1,
    VIRTIO_PCI_CAP_NOTIFY_CFG = 2,
    VIRTIO_PCI_CAP_ISR_CFG = 3, 
    VIRTIO_PCI_CAP_DEVICE_CFG = 4,
    VIRTIO_PCI_CAP_PCI_CFG = 5, 
    VIRTIO_PCI_CAP_SHARED_MEMORY_CFG = 8,
} 

impl From<CfgType> for u8{
    fn from(val: CfgType) -> u8 {
        match val {
            CfgType::INVALID => 0,
            CfgType::VIRTIO_PCI_CAP_COMMON_CFG => 1,
            CfgType::VIRTIO_PCI_CAP_NOTIFY_CFG => 2,
            CfgType::VIRTIO_PCI_CAP_ISR_CFG => 3,
            CfgType::VIRTIO_PCI_CAP_DEVICE_CFG => 4,
            CfgType::VIRTIO_PCI_CAP_PCI_CFG => 5,
            CfgType::VIRTIO_PCI_CAP_SHARED_MEMORY_CFG => 8,
        }
    }
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
            _ => CfgType::INVALID,
        }
    }
}
 
/// Public structure to allow drivers to read the configuration space 
/// savely
pub struct Origin {
    cfg_ptr: Le32, // Register to be read to reach configuration structure of type cfg_type
    dev: u8, // PCI device this configuration comes from
    bus: u8, // Bus of the PCI device
    dev_id: u16,
    cap_struct: PciCapRaw,
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
    bar: PciBar,
    id: u8,
    offset: Offset,
    length: Le32,
    // Following field can be used to retrieve original structure 
    // from the config space. Needed by some structures and f
    // device specific configs.
    origin: Origin,
}

impl PciCap {
    fn get_type(&self) -> &CfgType{
        &self.cfg_type
    }

    fn get_id(&self) -> u8 {
        self.id
    }
}

/// Virtio's PCI capabilites structure. 
/// See Virtio specification v.1.1 - 4.1.4
///
/// WARN: endianness of this structure should be seen as little endian.
/// As this structure is not meant to be used outside of this module and for
/// ease of conversion from reading data into struct from PCI configuration
/// space, no conversion is made for struct fields.
#[repr(C)]
struct PciCapRaw {
    cap_vndr: u8, 
    cap_next: u8, 
    cap_len: u8, 
    cfg_type: u8,
    bar_index: u8,
    id: u8,
    padding: [u8; 2],
    offset: Le32,
    length: Le32,
}

// This only shows compiler, that structs are identical 
// with themselves.
impl Eq for PciCapRaw {}

// In order to compare two PciCapRaw structs PartialEq is needed
impl PartialEq for PciCapRaw {
    fn eq(&self, other: &Self) -> bool {
        if self.cap_vndr == other.cap_vndr &&
            self.cap_next == other.cap_next &&
            self.cap_len == other.cap_len &&
            self.cfg_type == other.cfg_type &&
            self.bar_index == other.bar_index &&
            self.id == other.id &&
            self.offset == other.offset &&
            self.length == other.length {
                true
            } else {
                false
            }
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
    fn new() -> Self {
        UniCapsColl {
            com_cfg_list: Vec::new(),
            notif_cfg_list: Vec::new(),
            isr_stat_list: Vec::new(),
            pci_cfg_acc_list: Vec::new(),
            sh_mem_cfg_list: Vec::new(),
            dev_cfg_list: Vec::new(),
        }
    }

    fn add_cfg_common(&mut self, com: ComCfg) {
        self.com_cfg_list.push(com);
        // Resort array
        // 
        // This should not be to expensive, as "rational" devices will hold an
        // acceptibal amount of configuration structures.
        self.com_cfg_list.sort_by(|a, b| a.rank.cmp(&b.rank));
    }

    fn add_cfg_notif(&mut self, notif: NotifCfg) {
        self.notif_cfg_list.push(notif);
        // Resort array
        // 
        // This should not be to expensive, as "rational" devices will hold an
        // acceptable amount of configuration structures.
        self.notif_cfg_list.sort_by(|a, b| a.rank.cmp(&b.rank));
    }

    fn add_cfg_isr(&mut self, isr_raw: IsrStatusRaw, rank: u8) {
        self.isr_stat_list.push(IsrStatus::new(isr_raw, rank));
        // Resort array
        // 
        // This should not be to expensive, as "rational" devices will hold an
        // acceptable amount of configuration structures.
        self.isr_stat_list.sort_by(|a, b| a.rank.cmp(&b.rank));
    }

    fn add_cfg_pci(&mut self, pci_raw: PciCfgRaw, rank: u8) {
        self.pci_cfg_acc_list.push(PciCfg::new(pci_raw, rank));
        // Resort array
        // 
        // This should not be to expensive, as "rational" devices will hold an
        // acceptable amount of configuration structures.
        self.pci_cfg_acc_list.sort_by(|a, b| a.rank.cmp(&b.rank));
    }

    fn add_cfg_sh_mem(&mut self, sh_mem_raw: ShMemCfgRaw, id: u8) {
        self.sh_mem_cfg_list.push(ShMemCfg::new(sh_mem_raw, id));
        // Resort array
        // 
        // This should not be to expensive, as "rational" devices will hold an
        // acceptable amount of configuration structures.
        self.sh_mem_cfg_list.sort_by(|a, b| a.id.cmp(&b.id));
    }

    fn add_cfg_dev(&mut self, pci_cap: PciCap) {
        self.dev_cfg_list.push(pci_cap);
        // Resort array
        // 
        // This should not be to expensive, as "rational" devices will hold an
        // acceptable amount of configuration structures.
        self.dev_cfg_list.sort_by(|a, b| a.id.cmp(&b.id));
    }
}

/// Wraps a [ComCfgRaw](structs.comcfgraw.html) in order to preserve
/// the original structure.
///
/// Provides a safe API for Raw structure and allows interaction with the device via 
/// the structure. 
pub struct ComCfg {
    com_cfg: Box<ComCfgRaw>,
    rank: u8,
}

impl ComCfg {
    fn new(raw: Box<ComCfgRaw>, rank: u8) -> Self {
        ComCfg{
            com_cfg: raw,
            rank,
        }
    }

    pub fn write_vq_mem_areas(&self) {
        unimplemented!();
    }

    fn set_vq_desc_area(raw_cfg: &ComCfgRaw, desc_addr: PhyMemAddr, vq_id: Le16,) {
        unimplemented!();
    }

    fn set_vq_dev_area(raw_cfg: &ComCfgRaw, dev_ddr: PhyMemAddr, vq_id: Le16) {
        unimplemented!();
    }

    fn set_vq_driv_area(raw_cfg: &ComCfgRaw, desc_addr: PhyMemAddr, vq_id: Le16) {
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
    /// Returns a boxed [ComCfgRaw](ComCfgRaw) structure. The box points to the actual structure inside the 
    /// PCI devices memory space.
    fn map(cap: &PciCap) -> Option<Box<Self>> {
        if cap.bar.length <  u64::from(u32::from(cap.offset) + cap.length.as_ne()) {
            error!("Common config of with id {}, does not fit into memeory specified by bar {:x}!", cap.id, cap.bar.index);
            return None
        }

        // Using "as u32" is safe here as ComCfgRaw has a defined size smaller 2^31-1
        // Drivers MAY do this check. See Virtio specification v1.1. - 4.1.4.1
        if cap.length.as_ne()/8 < mem::size_of::<ComCfgRaw>() as u32 {
            error!("Common config of with id {}, does not represent actual structure specified by the standard!", cap.id);
            return None 
        }

        let virt_addr_raw = cap.bar.mem_addr + cap.offset;

        let com_cfg_raw: Box<ComCfgRaw>= unsafe {
            let raw = usize::from(virt_addr_raw) as *mut ComCfgRaw;
            Box::from_raw(raw)
        };

        Some(com_cfg_raw)
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

/// Notification Structure to handle virtqueue notification settings.
/// See Virtio specification v1.1 - 4.1.4.4 
// 
pub struct NotifCfg {
    base_addr: VirtMemAddr,
    notify_off_multiplier: Le32,
    rank: u8,
    // defines the maximum size of the notification space, starting from base_addr.
    length: Le32,
}

impl NotifCfg {
    fn new(cap: &PciCap) -> Option<Self> {
        if cap.bar.length <  u64::from(u32::from(cap.offset) + cap.length.as_ne()) {
            error!("Notification config of device {:x}, does not fit into memeory specified by bar {:x}!", cap.origin.dev_id, cap.bar.index);
            return None
        }

        // Assumes the cap_len is a multiple of 8 
        // This read MIGHT be slow, as it does NOT ensure 32 bit alignment.
        let notify_off_multiplier = Le32::from(env::pci::read_cfg_no_adapter(
            cap.origin.bus, 
            cap.origin.bus,
            cap.origin.cfg_ptr + Le32::from(cap.origin.cap_struct.cap_len))
        );

        // define base memory address from which the actuall Queue Notify address can be derived via
        // base_addr + queue_notify_off * notify_off_multiplier.
        // 
        // Where queue_notify_off is taken from the respective common configuration struct. 
        // See Virtio specification v1.1. - 4.1.4.4
        //
        // Base address here already includes offset!
        let base_addr = VirtMemAddr::from(cap.bar.mem_addr + cap.offset);

        Some(NotifCfg{
            base_addr: base_addr,
            notify_off_multiplier,
            rank: cap.id,
            length: cap.length
        })
    }
    
    //
    // THIS IS PLACEHOLDER JUST TO GET THE IDEA RIGHT
    //
    fn write(&self, queue_notif_off: u32) {
        unimplemented!();
        // Write to memory address = self.base_addr + queue_notif_off * self.notifiy_off_multiplier
        //
        // WHAT TO WRITE IS THE QUESTION?
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
#[repr(C)]
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
#[repr(C)]
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
#[repr(C)]
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

/// PciBar stores the virtual memory address and associated length of memory space
/// a PCI device's physical memory indicated by the device's BAR has been mapped to.
//
// Currently all fields are public as the struct is instanciated in the drivers::virtio::env module
#[derive(Clone, Debug)]
pub struct PciBar {
    index: u8,
    mem_addr: VirtMemAddr,
    length: u64,
}

impl PciBar {
    pub fn new(index: u8, mem_addr: VirtMemAddr, length: u64) -> Self {
        PciBar {
            index, 
            mem_addr,
            length,
        }
    }
}

/// Reads a raw capability struct [PciCapRaw](structs.PcicapRaw.html) out of a PCI device's configuration space.
fn read_cap_raw(adapter: &PciAdapter, register: Le32) -> PciCapRaw {
    let mut quadruple_word: [u8; 16] = [0; 16];

    debug!("Converting read word from PCI device config space into native endian bytes.");

    // Write words sequentialy into array
    let mut index = 0;
    for i in 0..4 {
        // Read word need to be converted to little endian bytes as PCI is little endian. 
        // Intepretation of multi byte values needs to be swapped for big endian machines
        let word: [u8; 4] = env::pci::read_config(adapter, Le32::from(register + Le32::from(4*i as u32))).to_le_bytes();
        for j in 0..4 {
            quadruple_word[index] = word[j];
            index += 1;
        }
    }

    PciCapRaw {
        cap_vndr: quadruple_word[0],
        cap_next: quadruple_word[1],
        cap_len: quadruple_word[2],
        cfg_type: quadruple_word[3],
        bar_index: quadruple_word[4],
        id: quadruple_word[5],
        // Unwrapping is okay here, as transformed array slice is always 2 * u8 long and initalized
        padding: quadruple_word[6..8].try_into().unwrap(),
        // Unwrapping is okay here, as transformed array slice is always 4 * u8 long and initalized
        offset: Le32::from(u32::from_le_bytes(quadruple_word[8..12].try_into().unwrap())),
        length: Le32::from(u32::from_le_bytes(quadruple_word[12..16].try_into().unwrap())),
    }
}

/// Reads all PCI capabilities, starting at the capabilites list pointer from the 
/// PCI device. 
///
/// Returns ONLY Virtio specific capabilites, which allow to locate the actual capability 
/// structures inside the memory areas, indicated by the BaseAddressRegisters (BAR's).
fn read_caps(adapter: &PciAdapter, bars: Vec<PciBar>) -> Result<Vec<PciCap>, PciError> {
    // Checks if pointer is well formed and does not point into config header space
    let ptr=  dev_caps_ptr(adapter);

    let mut next_ptr =  if ptr >= Le32::from(0x40u32) { 
        ptr
    } else {
       return Err(PciError::BadCapPtr(adapter.device_id))
    };

    let mut cap_list: Vec<PciCap> = Vec::new();
    // Loop through capabilties list via next pointer
    'cap_list: while next_ptr != Le32::from(0u32) {
        // read into raw capabilities structure
        //
        // Devices configuration space muste be read twice
        // and only returns correct values if both reads
        // return equal values.
        // For clarity see Virtio specification v1.1. - 2.4.1
        let mut before = read_cap_raw(adapter, next_ptr);
        let mut cap_raw = read_cap_raw(adapter, next_ptr);
    
        while before != cap_raw {
            before = read_cap_raw(adapter, next_ptr);
            cap_raw = read_cap_raw(adapter, next_ptr);
        }

        let mut iter = bars.iter();

        // Virtio specification v1.1. - 4.1.4 defines virtio specific capability
        // with virtio vendor id = 0x09
        match cap_raw.cap_vndr {
            0x09u8 => {
                let cap_bar: PciBar = loop {
                    match iter.next() {
                        Some(bar) => {
                            // Drivers MUST ignore BAR values different then specified in Virtio spec v1.1. - 4.1.4
                            // See Virtio specification v1.1. - 4.1.4.1
                            if bar.index <= 5 { 
                                if bar.index == cap_raw.bar_index {
                                    // Need to clone here as every PciCap carrys it's bar
                                    break bar.clone();
                                }  
                            } 
                        },
                        None => {
                            error!("Found virtio capability whose BAR is not mapped or non existing. Capability of type {:x} and id {:x} for device {:x}, can not be used!",
                                cap_raw.cfg_type, cap_raw.id, adapter.device_id);
                            
                            next_ptr = Le32::from(u32::from(cap_raw.cap_next));
                            continue 'cap_list;
                        },
                    }
                };
                // Need to set next_ptr inside first match as it will be moved inside PciCap
                next_ptr = Le32::from(u32::from(cap_raw.cap_next));

                cap_list.push(PciCap{
                    cfg_type: CfgType::from(cap_raw.cfg_type),
                    bar: cap_bar,
                    id: cap_raw.id,
                    offset: Offset::from(cap_raw.offset.as_ne()),
                    length: cap_raw.length,
                    origin: Origin {
                        cfg_ptr: next_ptr,
                        dev: adapter.device,
                        bus: adapter.bus,
                        dev_id: adapter.device_id,
                        cap_struct: cap_raw
                    },
                })
            }
            _ => next_ptr = Le32::from(u32::from(cap_raw.cap_next)),
        }
    }

    if cap_list.is_empty() {
        error!("No virtio capability found for device {:x}", adapter.device_id);
        Err(PciError::NoVirtioCaps(adapter.device_id))
    } else {
        Ok(cap_list)
    }
}

/// Wrapper function to get a devices current status.
/// As the device is not static, return value is not static.
fn dev_status(adapter: &PciAdapter) -> u32 {
    env::pci::read_config(adapter, Le32::from(u32::from(constants::RegisterHeader00H::PCI_COMMAND_REGISTER))) >> 16
}

/// Wrapper function to get a devices capabilites list pointer, which represents
/// an offset starting from the header of the device's configuration space.
fn dev_caps_ptr(adapter: &PciAdapter) -> Le32 {
    Le32::from(
        env::pci::read_config(adapter, Le32::from(u32::from(constants::RegisterHeader00H::PCI_CAPABILITY_LIST_REGISTER))) 
        & u32::from(constants::Masks::PCI_MASK_CAPLIST_POINTER)
    )
}

/// Maps memory areas indicated by devices BAR's into virtual address space.
fn map_bars(adapter: &PciAdapter) -> Result<Vec<PciBar>, PciError> {
    crate::drivers::virtio::env::pci::map_bar_mem(adapter) 
}

/// Checks if the status of the device inidactes the device is using the 
/// capabilites pointer and therefore defines a capabiites list.
fn no_cap_list(adapter: &PciAdapter) -> bool {
    dev_status(adapter) & u32::from(constants::Masks::PCI_MASK_STATUS_CAPABILITIES_LIST) == 0
}

/// Checks if minimal set of capabilities is present.
fn check_caps(caps: UniCapsColl) -> Result<UniCapsColl, PciError> {
    unimplemented!();
}

pub fn map_caps(adapter: &PciAdapter) -> Result<UniCapsColl, PciError> {
    // In case caplist pointer is not used, abort as it is essential
    if  no_cap_list(adapter) {
		error!("Found virtio device without capability list. Aborting!");
		return Err(PciError::NoCapPtr(adapter.device_id));
    }

    // Mapped memory areas are reachable through PciBar structs.
    let bar_list = match map_bars(adapter) {
        Ok(list) => list,
        Err(pci_error) => {
            return Err(pci_error)
        },
    };

    // Get list of PciCaps pointing to capabilities
    let cap_list =  match read_caps(adapter, bar_list) {
        Ok(list) => list, 
        Err(pci_error) => {
            return Err(pci_error)
        },
    };

    let mut caps = UniCapsColl::new();
    // Map Caps in virtual memory
    for pci_cap in cap_list {
        match pci_cap.get_type() {
            CfgType::VIRTIO_PCI_CAP_COMMON_CFG =>  match ComCfgRaw::map(&pci_cap) {
                Some(cap) => caps.add_cfg_common(ComCfg::new(cap, pci_cap.get_id())),
                None => error!("Common config capability with id {}, of device {:x}, could not be mapped!", pci_cap.id, adapter.device_id),
            },
            CfgType::VIRTIO_PCI_CAP_NOTIFY_CFG => match NotifCfg::new(&pci_cap) {
                Some(notif) => caps.add_cfg_notif(notif),
                None => error!("Notification config capability with id {}, of device {:X} could not be used!", pci_cap.id, adapter.device_id),
            },
            CfgType::VIRTIO_PCI_CAP_ISR_CFG => caps.add_cfg_isr(IsrStatusRaw::map(&pci_cap), pci_cap.get_id()),
            CfgType::VIRTIO_PCI_CAP_PCI_CFG => caps.add_cfg_pci(PciCfgRaw::map(&pci_cap), pci_cap.get_id()),
            CfgType::VIRTIO_PCI_CAP_SHARED_MEMORY_CFG => caps.add_cfg_sh_mem(ShMemCfgRaw::map(&pci_cap), pci_cap.get_id()),
            CfgType::VIRTIO_PCI_CAP_DEVICE_CFG => caps.add_cfg_dev(pci_cap),
            // PCI's configuration space is allowed to hold other structures, which are not virtio specific and are therefore ignored
            // in the following
            _ => continue,
        }
    }
    
    check_caps(caps)
}

/// Checks existing drivers for support of given device. Upon match, provides
/// driver with a [Caplist](struct.Caplist.html) struct, holding the structures of the capabilities
/// list of the given device.
pub fn init_device(adapter: &PciAdapter) -> Result<PciDriver, DriverError> {
    match DevId::from(adapter.device_id) {
        DevId::VIRTIO_TRANS_DEV_ID_NET | 
        DevId::VIRTIO_TRANS_DEV_ID_BLK | 
        DevId::VIRTIO_TRANS_DEV_ID_MEM_BALL   |
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
                    error!(
                        "Virtio networkd driver could not be initalized with device: {:x}",
                        adapter.device_id
                    );
                    return Err(DriverError::InitVirtioDevFail(virtio_error))
                },
            };
        },
		DevId::VIRTIO_DEV_ID_FS => {
		    // TODO: Proper error handling in driver creation fail
            // virtio_fs::create_virtiofs_driver(adapter).unwrap()

            // PLACEHOLDER TO GET RIGHT RETURNvi    
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

/// WILL BE MERGED INTO AN ENUM OR AS CONSTANT
/// PROBABLY SHOULD ME MERGED INTO kernel_PCI code
pub mod constants {
    // 
    // ALL CONSTANTS MUST BE MADE TO LITTLE ENDIAN!
    //
    pub const PCI_MAX_BUS_NUMBER: u8 = 32;
    pub const PCI_MAX_DEVICE_NUMBER: u8 = 32;
    pub const PCI_CONFIG_ADDRESS_PORT: u16 = 0xCF8;
    pub const PCI_CONFIG_ADDRESS_ENABLE: u32 = 1 << 31;
    pub const PCI_CONFIG_DATA_PORT: u16 = 0xCFC;
    pub const PCI_COMMAND_BUSMASTER: u32 = 1 << 2;
    pub const PCI_BASE_ADDRESS_IO_SPACE: u32 = 1 << 0;
    pub const PCI_MEM_BASE_ADDRESS_64BIT: u32 = 1 << 2;
    pub const PCI_MEM_PREFETCHABLE: u32 = 1 << 3;
    pub const PCI_CAP_ID_VNDR: u32 = 0x09;  

    /// PCI registers offset inside header,
    /// if PCI header is of type 00h.
    #[allow(dead_code, non_camel_case_types)]
    #[repr(u32)]
    pub enum RegisterHeader00H {
        PCI_ID_REGISTER = 0x00u32,
        PCI_COMMAND_REGISTER  = 0x04u32,
        PCI_CLASS_REGISTER = 0x08u32,
        PCI_HEADER_REGISTER  = 0x0Cu32,
        PCI_BAR0_REGISTER  = 0x10u32,
        PCI_CAPABILITY_LIST_REGISTER = 0x34u32,
        PCI_INTERRUPT_REGISTER = 0x3Cu32,
    }

    impl From<RegisterHeader00H> for u32 {
        fn from(val: RegisterHeader00H) -> u32 {
             match val {
                RegisterHeader00H::PCI_ID_REGISTER => 0x00u32,
                RegisterHeader00H::PCI_COMMAND_REGISTER => 0x04u32,
                RegisterHeader00H::PCI_CLASS_REGISTER => 0x08u32,
                RegisterHeader00H::PCI_HEADER_REGISTER => 0x0Cu32,
                RegisterHeader00H::PCI_BAR0_REGISTER => 0x10u32,
                RegisterHeader00H::PCI_CAPABILITY_LIST_REGISTER => 0x34u32,
                RegisterHeader00H::PCI_INTERRUPT_REGISTER => 0x3Cu32,
            }
        }
    }

    /// PCI masks. For convenience put into an enum and provides
    /// an Into<u32> method for usage.
    #[allow(dead_code, non_camel_case_types)]
    #[repr(u32)]
    pub enum Masks {
        PCI_MASK_STATUS_CAPABILITIES_LIST = 0x0000_0010u32,
        PCI_MASK_CAPLIST_POINTER = 0x0000_00FCu32,
        PCI_MASK_HEADER_TYPE = 0x007F_0000u32,
        PCI_MASK_MULTIFUNCTION = 0x0080_0000u32,
        PCI_MASK_MEM_BASE_ADDRESS = 0xFFFF_FFF0u32,
        PCI_MASK_IO_BASE_ADDRESS = 0xFFFF_FFFCu32,
    }

    impl From<Masks> for u32 {
        fn from(val: Masks) -> u32 {
            match val {
                Masks::PCI_MASK_STATUS_CAPABILITIES_LIST => 0x0000_0010u32,
                Masks::PCI_MASK_CAPLIST_POINTER => 0x0000_00FCu32,
                Masks::PCI_MASK_HEADER_TYPE => 0x007F_0000u32,
                Masks::PCI_MASK_MULTIFUNCTION => 0x0080_0000u32,
                Masks::PCI_MASK_MEM_BASE_ADDRESS => 0xFFFF_FFF0u32,
                Masks::PCI_MASK_IO_BASE_ADDRESS => 0xFFFF_FFFCu32,
            } 
        }
    }
}