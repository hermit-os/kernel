//! Definitions for Virtio over PCI bus.

use volatile::access::ReadOnly;
use volatile::VolatileFieldAccess;

use crate::num::*;
use crate::DeviceStatus;

/// Common configuration structure
///
/// The common configuration structure is found at the bar and offset within the [`VIRTIO_PCI_CAP_COMMON_CFG`] capability.
#[doc(alias = "virtio_pci_common_cfg")]
#[cfg_attr(
    feature = "zerocopy",
    derive(zerocopy_derive::FromZeroes, zerocopy_derive::FromBytes)
)]
#[derive(VolatileFieldAccess)]
#[allow(non_camel_case_types)]
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
    queue_desc: le64,

    /// The driver writes the physical address of Driver Area here.  See section _Basic Facilities of a Virtio Device / Virtqueues_.
    queue_driver: le64,

    /// The driver writes the physical address of Device Area here.  See section _Basic Facilities of a Virtio Device / Virtqueues_.
    queue_device: le64,

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
    /// [`VIRTIO_F_NOTIF_CONFIG_DATA`]: crate::features::VirtioF::NOTIF_CONFIG_DATA
    #[access(ReadOnly)]
    queue_notify_data: le16,

    /// The driver uses this to selectively reset the queue.
    /// This field exists only if [`VIRTIO_F_RING_RESET`] has been
    /// negotiated. (see _Basic Facilities of a Virtio Device / Virtqueues / Virtqueue Reset_).
    ///
    /// [`VIRTIO_F_RING_RESET`]: crate::features::VirtioF::RING_RESET
    queue_reset: le16,
}
