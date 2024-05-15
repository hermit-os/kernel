//! Feature Bits

use bitflags::bitflags;

/// Device-independent Feature Bits
#[doc(alias = "VIRTIO_F")]
#[cfg_attr(
    feature = "zerocopy",
    derive(
        zerocopy_derive::FromZeroes,
        zerocopy_derive::FromBytes,
        zerocopy_derive::AsBytes
    )
)]
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct VirtioF(u128);

bitflags_debug!(VirtioF);

bitflags! {
    impl VirtioF: u128 {
        /// Negotiating this feature indicates
        /// that the driver can use descriptors with the VIRTQ_DESC_F_INDIRECT
        /// flag set, as described in _Basic Facilities of a Virtio
        /// Device / Virtqueues / The Virtqueue Descriptor Table / Indirect
        /// Descriptors_ _Basic Facilities of a Virtio Device /
        /// Virtqueues / The Virtqueue Descriptor Table / Indirect
        /// Descriptors_ and _Packed Virtqueues / Indirect Flag: Scatter-Gather Support_ _Packed Virtqueues / Indirect Flag: Scatter-Gather Support_.
        #[doc(alias = "VIRTIO_F_INDIRECT_DESC")]
        const INDIRECT_DESC = 1 << 28;

        /// This feature enables the _used_event_
        /// and the _avail_event_ fields as described in
        /// _Basic Facilities of a Virtio Device / Virtqueues / Used Buffer Notification Suppression_, _Basic Facilities of a Virtio Device / Virtqueues / The Virtqueue Used Ring_ and _Packed Virtqueues / Driver and Device Event Suppression_.
        #[doc(alias = "VIRTIO_F_EVENT_IDX")]
        const EVENT_IDX = 1 << 29;

        /// This indicates compliance with this
        /// specification, giving a simple way to detect legacy devices or drivers.
        #[doc(alias = "VIRTIO_F_VERSION_1")]
        const VERSION_1 = 1 << 32;

        /// This feature indicates that
        /// the device can be used on a platform where device access to data
        /// in memory is limited and/or translated. E.g. this is the case if the device can be located
        /// behind an IOMMU that translates bus addresses from the device into physical
        /// addresses in memory, if the device can be limited to only access
        /// certain memory addresses or if special commands such as
        /// a cache flush can be needed to synchronise data in memory with
        /// the device. Whether accesses are actually limited or translated
        /// is described by platform-specific means.
        /// If this feature bit is set to 0, then the device
        /// has same access to memory addresses supplied to it as the
        /// driver has.
        /// In particular, the device will always use physical addresses
        /// matching addresses used by the driver (typically meaning
        /// physical addresses used by the CPU)
        /// and not translated further, and can access any address supplied to it by
        /// the driver. When clear, this overrides any platform-specific description of
        /// whether device access is limited or translated in any way, e.g.
        /// whether an IOMMU may be present.
        #[doc(alias = "VIRTIO_F_ACCESS_PLATFORM")]
        const ACCESS_PLATFORM = 1 << 33;

        /// This feature indicates
        /// support for the packed virtqueue layout as described in
        /// _Basic Facilities of a Virtio Device / Packed Virtqueues_ _Basic Facilities of a Virtio Device / Packed Virtqueues_.
        #[doc(alias = "VIRTIO_F_RING_PACKED")]
        const RING_PACKED = 1 << 34;

        /// This feature indicates
        /// that all buffers are used by the device in the same
        /// order in which they have been made available.
        #[doc(alias = "VIRTIO_F_IN_ORDER")]
        const IN_ORDER = 1 << 35;

        /// This feature indicates
        /// that memory accesses by the driver and the device are ordered
        /// in a way described by the platform.
        ///
        /// If this feature bit is negotiated, the ordering in effect for any
        /// memory accesses by the driver that need to be ordered in a specific way
        /// with respect to accesses by the device is the one suitable for devices
        /// described by the platform. This implies that the driver needs to use
        /// memory barriers suitable for devices described by the platform; e.g.
        /// for the PCI transport in the case of hardware PCI devices.
        ///
        /// If this feature bit is not negotiated, then the device
        /// and driver are assumed to be implemented in software, that is
        /// they can be assumed to run on identical CPUs
        /// in an SMP configuration.
        /// Thus a weaker form of memory barriers is sufficient
        /// to yield better performance.
        #[doc(alias = "VIRTIO_F_ORDER_PLATFORM")]
        const ORDER_PLATFORM = 1 << 36;

        /// This feature indicates that
        /// the device supports Single Root I/O Virtualization.
        /// Currently only PCI devices support this feature.
        #[doc(alias = "VIRTIO_F_SR_IOV")]
        const SR_IOV = 1 << 37;

        /// This feature indicates
        /// that the driver passes extra data (besides identifying the virtqueue)
        /// in its device notifications.
        /// See _Virtqueues / Driver notifications_ _Virtqueues / Driver notifications_.
        #[doc(alias = "VIRTIO_F_NOTIFICATION_DATA")]
        const NOTIFICATION_DATA = 1 << 38;

        /// This feature indicates that the driver
        /// uses the data provided by the device as a virtqueue identifier in available
        /// buffer notifications.
        /// As mentioned in section _Virtqueues / Driver notifications_, when the
        /// driver is required to send an available buffer notification to the device, it
        /// sends the virtqueue number to be notified. The method of delivering
        /// notifications is transport specific.
        /// With the PCI transport, the device can optionally provide a per-virtqueue value
        /// for the driver to use in driver notifications, instead of the virtqueue number.
        /// Some devices may benefit from this flexibility by providing, for example,
        /// an internal virtqueue identifier, or an internal offset related to the
        /// virtqueue number.
        ///
        /// This feature indicates the availability of such value. The definition of the
        /// data to be provided in driver notification and the delivery method is
        /// transport specific.
        /// For more details about driver notifications over PCI see _Virtio Transport Options / Virtio Over PCI Bus / PCI-specific Initialization And Device Operation / Available Buffer Notifications_.
        #[doc(alias = "VIRTIO_F_NOTIF_CONFIG_DATA")]
        const NOTIF_CONFIG_DATA = 1 << 39;

        /// This feature indicates
        /// that the driver can reset a queue individually.
        /// See _Basic Facilities of a Virtio Device / Virtqueues / Virtqueue Reset_.
        #[doc(alias = "VIRTIO_F_RING_RESET")]
        const RING_RESET = 1 << 40;

        const _ = !0;
    }
}
