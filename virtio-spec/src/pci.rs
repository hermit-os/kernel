//! Definitions for Virtio over PCI bus.

use core::mem;

use endian_num::{le64, Le};
use num_enum::{FromPrimitive, IntoPrimitive};
use pci_types::capability::PciCapabilityAddress;
use pci_types::ConfigRegionAccess;
use volatile::access::{ReadOnly, ReadWrite, RestrictAccess};
use volatile::VolatilePtr;
use volatile_macro::VolatileFieldAccess;

use crate::volatile::WideVolatilePtr;
use crate::{le16, le32, DeviceStatus};

/// PCI Capability
///
/// See [`CapData`] for reading additional fields.
#[doc(alias = "virtio_pci_cap")]
#[cfg_attr(
    feature = "zerocopy",
    derive(zerocopy_derive::FromZeroes, zerocopy_derive::FromBytes)
)]
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct Cap {
    /// Generic PCI field: `PCI_CAP_ID_VNDR`
    ///
    /// 0x09; Identifies a vendor-specific capability.
    pub cap_vndr: u8,

    /// Generic PCI field: next ptr.
    ///
    /// Link to next capability in the capability list in the PCI configuration space.
    pub cap_next: u8,

    /// Generic PCI field: capability length
    ///
    /// Length of this capability structure, including the whole of
    /// struct virtio_pci_cap, and extra data if any.
    /// This length MAY include padding, or fields unused by the driver.
    pub cap_len: u8,

    /// Identifies the structure.
    ///
    /// Each structure is detailed individually below.
    ///
    /// The device MAY offer more than one structure of any type - this makes it
    /// possible for the device to expose multiple interfaces to drivers.  The order of
    /// the capabilities in the capability list specifies the order of preference
    /// suggested by the device.  A device may specify that this ordering mechanism be
    /// overridden by the use of the `id` field.
    ///
    /// <div class="warning">
    ///
    /// For example, on some hypervisors, notifications using IO accesses are
    /// faster than memory accesses. In this case, the device would expose two
    /// capabilities with `cfg_type` set to VIRTIO_PCI_CAP_NOTIFY_CFG:
    /// the first one addressing an I/O BAR, the second one addressing a memory BAR.
    /// In this example, the driver would use the I/O BAR if I/O resources are available, and fall back on
    /// memory BAR when I/O resources are unavailable.
    ///
    /// </div>
    pub cfg_type: u8,

    /// Where to find it.
    ///
    /// values 0x0 to 0x5 specify a Base Address register (BAR) belonging to
    /// the function located beginning at 10h in PCI Configuration Space
    /// and used to map the structure into Memory or I/O Space.
    /// The BAR is permitted to be either 32-bit or 64-bit, it can map Memory Space
    /// or I/O Space.
    ///
    /// Any other value is reserved for future use.
    pub bar: u8,

    /// Multiple capabilities of the same type
    ///
    /// Used by some device types to uniquely identify multiple capabilities
    /// of a certain type. If the device type does not specify the meaning of
    /// this field, its contents are undefined.
    pub id: u8,

    /// Pad to full dword.
    pub padding: [u8; 2],

    /// Offset within bar.
    ///
    /// indicates where the structure begins relative to the base address associated
    /// with the BAR.  The alignment requirements of `offset` are indicated
    /// in each structure-specific section below.
    pub offset: le32,

    /// Length of the structure, in bytes.
    ///
    /// indicates the length of the structure.
    ///
    /// `length` MAY include padding, or fields unused by the driver, or
    /// future extensions.
    ///
    /// <div class="warning">
    ///
    /// For example, a future device might present a large structure size of several
    /// MBytes.
    /// As current devices never utilize structures larger than 4KBytes in size,
    /// driver MAY limit the mapped structure size to e.g.
    /// 4KBytes (thus ignoring parts of structure after the first
    /// 4KBytes) to allow forward compatibility with such devices without loss of
    /// functionality and without wasting resources.
    ///
    /// </div>
    pub length: le32,
}

impl Cap {
    pub fn read(addr: PciCapabilityAddress, access: &impl ConfigRegionAccess) -> Option<Self> {
        let data = unsafe { access.read(addr.address, addr.offset) };
        let [cap_vndr, _cap_next, cap_len, _cfg_type] = data.to_ne_bytes();

        if cap_vndr != 0x09 {
            return None;
        }

        if cap_len < 16 {
            return None;
        }

        let data = [
            data,
            unsafe { access.read(addr.address, addr.offset + 4) },
            unsafe { access.read(addr.address, addr.offset + 8) },
            unsafe { access.read(addr.address, addr.offset + 12) },
        ];

        let this = unsafe { mem::transmute::<[u32; 4], Self>(data) };

        Some(this)
    }
}

/// PCI Capability 64
#[doc(alias = "virtio_pci_cap64")]
#[cfg_attr(
    feature = "zerocopy",
    derive(zerocopy_derive::FromZeroes, zerocopy_derive::FromBytes)
)]
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct Cap64 {
    pub cap: Cap,
    pub offset_hi: le32,
    pub length_hi: le32,
}

/// PCI Notify Capability
#[doc(alias = "virtio_pci_notify_cap")]
#[cfg_attr(
    feature = "zerocopy",
    derive(zerocopy_derive::FromZeroes, zerocopy_derive::FromBytes)
)]
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct NotifyCap {
    pub cap: Cap,

    /// Multiplier for queue_notify_off.
    pub notify_off_multiplier: le32,
}

/// PCI Configuration Capability
#[doc(alias = "virtio_pci_cfg_cap")]
#[cfg_attr(
    feature = "zerocopy",
    derive(zerocopy_derive::FromZeroes, zerocopy_derive::FromBytes)
)]
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct CfgCap {
    pub cap: Cap,

    /// Data for BAR access.
    pub pci_cfg_data: [u8; 4],
}

/// PCI Capability Data
#[derive(Clone, Copy, Debug)]
pub struct CapData {
    /// Identifies the structure.
    pub cfg_type: CapCfgType,

    /// Where to find it.
    pub bar: u8,

    /// Multiple capabilities of the same type
    pub id: u8,

    /// Offset within bar.
    pub offset: le64,

    /// Length of the structure, in bytes.
    pub length: le64,

    /// Multiplier for queue_notify_off.
    pub notify_off_multiplier: Option<le32>,
}

impl CapData {
    pub fn read(addr: PciCapabilityAddress, access: &impl ConfigRegionAccess) -> Option<Self> {
        let cap = Cap::read(addr.clone(), access)?;
        let cfg_type = CapCfgType::from(cap.cfg_type);

        let (offset, length) = match cfg_type {
            CapCfgType::SharedMemory => {
                if cap.cap_len < 24 {
                    return None;
                }

                let offset_hi = unsafe { access.read(addr.address, addr.offset + 16) };
                let offset_hi = Le(offset_hi);
                let offset = le64::from([cap.offset, offset_hi]);

                let length_hi = unsafe { access.read(addr.address, addr.offset + 20) };
                let length_hi = Le(length_hi);
                let length = le64::from([cap.length, length_hi]);

                (offset, length)
            }
            _ => (le64::from(cap.offset), le64::from(cap.length)),
        };

        let notify_off_multiplier = match cfg_type {
            CapCfgType::Notify => {
                if cap.cap_len < 20 {
                    return None;
                }

                let notify_off_multiplier = unsafe { access.read(addr.address, addr.offset + 16) };
                let notify_off_multiplier = Le(notify_off_multiplier);

                Some(notify_off_multiplier)
            }
            _ => None,
        };

        Some(Self {
            cfg_type,
            bar: cap.bar,
            id: cap.id,
            offset,
            length,
            notify_off_multiplier,
        })
    }
}

/// PCI Capability Configuration Type
#[derive(IntoPrimitive, FromPrimitive, PartialEq, Eq, Clone, Copy, Debug)]
#[non_exhaustive]
#[repr(u8)]
pub enum CapCfgType {
    /// Common configuration
    #[doc(alias = "VIRTIO_PCI_CAP_COMMON_CFG")]
    Common = 1,

    /// Notifications
    #[doc(alias = "VIRTIO_PCI_CAP_NOTIFY_CFG")]
    Notify = 2,

    /// ISR Status
    #[doc(alias = "VIRTIO_PCI_CAP_ISR_CFG")]
    Isr = 3,

    /// Device specific configuration
    #[doc(alias = "VIRTIO_PCI_CAP_DEVICE_CFG")]
    Device = 4,

    /// PCI configuration access
    #[doc(alias = "VIRTIO_PCI_CAP_PCI_CFG")]
    Pci = 5,

    /// Shared memory region
    #[doc(alias = "VIRTIO_PCI_CAP_SHARED_MEMORY_CFG")]
    SharedMemory = 8,

    /// Vendor-specific data
    #[doc(alias = "VIRTIO_PCI_CAP_VENDOR_CFG")]
    Vendor = 9,

    /// Unknown device
    #[num_enum(catch_all)]
    Unknown(u8),
}

/// Common configuration structure
///
/// The common configuration structure is found at the bar and offset within the [`VIRTIO_PCI_CAP_COMMON_CFG`] capability.
///
/// [`VIRTIO_PCI_CAP_COMMON_CFG`]: Cap::CommonCfg
#[doc(alias = "virtio_pci_common_cfg")]
#[cfg_attr(
    feature = "zerocopy",
    derive(zerocopy_derive::FromZeroes, zerocopy_derive::FromBytes)
)]
#[derive(VolatileFieldAccess)]
#[repr(C)]
pub struct CommonCfg {
    /// The driver uses this to select which feature bits `device_feature` shows.
    /// Value 0x0 selects Feature Bits 0 to 31, 0x1 selects Feature Bits 32 to 63, etc.
    device_feature_select: le32,

    /// The device uses this to report which feature bits it is
    /// offering to the driver: the driver writes to
    /// `device_feature_select` to select which feature bits are presented.
    #[access(ReadOnly)]
    device_feature: le32,

    /// The driver uses this to select which feature bits `driver_feature` shows.
    /// Value 0x0 selects Feature Bits 0 to 31, 0x1 selects Feature Bits 32 to 63, etc.
    driver_feature_select: le32,

    /// The driver writes this to accept feature bits offered by the device.
    /// Driver Feature Bits selected by `driver_feature_select`.
    driver_feature: le32,

    /// The driver sets the Configuration Vector for MSI-X.
    config_msix_vector: le16,

    /// The device specifies the maximum number of virtqueues supported here.
    #[access(ReadOnly)]
    num_queues: le16,

    /// The driver writes the device status here (see [`DeviceStatus`]). Writing 0 into this
    /// field resets the device.
    device_status: DeviceStatus,

    /// Configuration atomicity value.  The device changes this every time the
    /// configuration noticeably changes.
    #[access(ReadOnly)]
    config_generation: u8,

    /// Queue Select. The driver selects which virtqueue the following
    /// fields refer to.
    queue_select: le16,

    /// Queue Size.  On reset, specifies the maximum queue size supported by
    /// the device. This can be modified by the driver to reduce memory requirements.
    /// A 0 means the queue is unavailable.
    queue_size: le16,

    /// The driver uses this to specify the queue vector for MSI-X.
    queue_msix_vector: le16,

    /// The driver uses this to selectively prevent the device from executing requests from this virtqueue.
    /// 1 - enabled; 0 - disabled.
    queue_enable: le16,

    /// The driver reads this to calculate the offset from start of Notification structure at
    /// which this virtqueue is located.
    ///
    /// <div class="warning">
    ///
    /// This is _not_ an offset in bytes.
    /// See _Virtio Transport Options / Virtio Over PCI Bus / PCI Device Layout / Notification capability_ below.
    ///
    /// </div>
    #[access(ReadOnly)]
    queue_notify_off: le16,

    /// The driver writes the physical address of Descriptor Area here.  See section _Basic Facilities of a Virtio Device / Virtqueues_.
    queue_desc_low: le32,

    /// The driver writes the physical address of Descriptor Area here.  See section _Basic Facilities of a Virtio Device / Virtqueues_.
    queue_desc_high: le32,

    /// The driver writes the physical address of Driver Area here.  See section _Basic Facilities of a Virtio Device / Virtqueues_.
    queue_driver_low: le32,

    /// The driver writes the physical address of Driver Area here.  See section _Basic Facilities of a Virtio Device / Virtqueues_.
    queue_driver_high: le32,

    /// The driver writes the physical address of Device Area here.  See section _Basic Facilities of a Virtio Device / Virtqueues_.
    queue_device_low: le32,

    /// The driver writes the physical address of Device Area here.  See section _Basic Facilities of a Virtio Device / Virtqueues_.
    queue_device_high: le32,

    /// This field exists only if [`VIRTIO_F_NOTIF_CONFIG_DATA`] has been negotiated.
    /// The driver will use this value to put it in the 'virtqueue number' field
    /// in the available buffer notification structure.
    /// See section _Virtio Transport Options / Virtio Over PCI Bus / PCI-specific Initialization And Device Operation / Available Buffer Notifications_.
    ///
    /// <div class="warning">
    ///
    /// This field provides the device with flexibility to determine how virtqueues
    /// will be referred to in available buffer notifications.
    /// In a trivial case the device can set `queue_notify_data`=vqn. Some devices
    /// may benefit from providing another value, for example an internal virtqueue
    /// identifier, or an internal offset related to the virtqueue number.
    ///
    /// </div>
    ///
    /// [`VIRTIO_F_NOTIF_CONFIG_DATA`]: crate::F::NOTIF_CONFIG_DATA
    #[access(ReadOnly)]
    queue_notify_data: le16,

    /// The driver uses this to selectively reset the queue.
    /// This field exists only if [`VIRTIO_F_RING_RESET`] has been
    /// negotiated. (see _Basic Facilities of a Virtio Device / Virtqueues / Virtqueue Reset_).
    ///
    /// [`VIRTIO_F_RING_RESET`]: crate::F::RING_RESET
    queue_reset: le16,
}

impl_wide_field_access! {
    /// Common configuration structure
    pub trait CommonCfgVolatileWideFieldAccess<'a, A>: CommonCfg {
        /// The driver writes the physical address of Device Area here.  See section _Basic Facilities of a Virtio Device / Virtqueues_.
        #[access(ReadWrite)]
        queue_desc: queue_desc_low, queue_desc_high;

        /// The driver writes the physical address of Device Area here.  See section _Basic Facilities of a Virtio Device / Virtqueues_.
        #[access(ReadWrite)]
        queue_driver: queue_driver_low, queue_driver_high;

        /// The driver writes the physical address of Device Area here.  See section _Basic Facilities of a Virtio Device / Virtqueues_.
        #[access(ReadWrite)]
        queue_device: queue_device_low, queue_device_high;
    }
}

virtio_bitflags! {
    /// ISR Status
    pub struct IsrStatus: u8 {
        /// Queue Interrupt
        const QUEUE_INTERRUPT = 1 << 0;

        /// Device Configuration Interrupt
        const DEVICE_CONFIGURATION_INTERRUPT = 1 << 1;
    }
}
