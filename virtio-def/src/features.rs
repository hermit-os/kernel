//! Feature Bits

virtio_bitflags! {
    /// Device-independent Feature Bits
    #[doc(alias = "VIRTIO_F")]
    pub struct VirtioF: u128 {
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

macro_rules! feature_bits {
    (
        $(#[$outer:meta])*
        $vis:vis struct $BitFlags:ident: $T:ty {
            $(
                $(#[$inner:ident $($args:tt)*])*
                const $Flag:tt = $value:expr;
            )*
        }

        $($t:tt)*
    ) => {
        virtio_bitflags! {
            $(#[$outer])*
            $vis struct $BitFlags: $T {
                $(
                    $(#[$inner $($args)*])*
                    const $Flag = $value;
                )*

                /// Device-independent Bit. See [`VirtioF::INDIRECT_DESC`].
                const INDIRECT_DESC = VirtioF::INDIRECT_DESC.bits();

                /// Device-independent Bit. See [`VirtioF::EVENT_IDX`].
                const EVENT_IDX = VirtioF::EVENT_IDX.bits();

                /// Device-independent Bit. See [`VirtioF::VERSION_1`].
                const VERSION_1 = VirtioF::VERSION_1.bits();

                /// Device-independent Bit. See [`VirtioF::ACCESS_PLATFORM`].
                const ACCESS_PLATFORM = VirtioF::ACCESS_PLATFORM.bits();

                /// Device-independent Bit. See [`VirtioF::RING_PACKED`].
                const RING_PACKED = VirtioF::RING_PACKED.bits();

                /// Device-independent Bit. See [`VirtioF::IN_ORDER`].
                const IN_ORDER = VirtioF::IN_ORDER.bits();

                /// Device-independent Bit. See [`VirtioF::ORDER_PLATFORM`].
                const ORDER_PLATFORM = VirtioF::ORDER_PLATFORM.bits();

                /// Device-independent Bit. See [`VirtioF::SR_IOV`].
                const SR_IOV = VirtioF::SR_IOV.bits();

                /// Device-independent Bit. See [`VirtioF::NOTIFICATION_DATA`].
                const NOTIFICATION_DATA = VirtioF::NOTIFICATION_DATA.bits();

                /// Device-independent Bit. See [`VirtioF::NOTIF_CONFIG_DATA`].
                const NOTIF_CONFIG_DATA = VirtioF::NOTIF_CONFIG_DATA.bits();

                /// Device-independent Bit. See [`VirtioF::RING_RESET`].
                const RING_RESET = VirtioF::RING_RESET.bits();
            }
        }

        impl From<VirtioF> for $BitFlags {
            fn from(value: VirtioF) -> Self {
                Self::from_bits_retain(value.bits())
            }
        }

        impl AsRef<$BitFlags> for VirtioF {
            fn as_ref(&self) -> &$BitFlags {
                unsafe { &*(self as *const Self as *const $BitFlags) }
            }
        }

        impl AsMut<$BitFlags> for VirtioF {
            fn as_mut(&mut self) -> &mut $BitFlags {
                unsafe { &mut *(self as *mut Self as *mut $BitFlags) }
            }
        }

        impl From<$BitFlags> for VirtioF {
            /// Returns the device-independent feature bits while retaining device-specific feature bits.
            fn from(value: $BitFlags) -> Self {
                VirtioF::from_bits_retain(value.bits())
            }
        }

        impl AsRef<VirtioF> for $BitFlags {
            /// Returns a shared reference to the device-independent features while retaining device-specific feature bits.
            fn as_ref(&self) -> &VirtioF {
                unsafe { &*(self as *const Self as *const VirtioF) }
            }
        }

        impl AsMut<VirtioF> for $BitFlags {
            /// Returns a mutable reference to the device-independent features while retaining device-specific feature bits.
            fn as_mut(&mut self) -> &mut VirtioF {
                unsafe { &mut *(self as *mut Self as *mut VirtioF) }
            }
        }

        feature_bits! {
            $($t)*
        }
    };
    () => {};
}

feature_bits! {
    /// Network Device Feature Bits
    #[doc(alias = "VIRTIO_NET_F")]
    pub struct VirtioNetF: u128 {
        /// Device handles packets with partial checksum.   This
        /// “checksum offload” is a common feature on modern network cards.
        #[doc(alias = "VIRTIO_NET_F_CSUM")]
        const CSUM = 1 << 0;

        /// Driver handles packets with partial checksum.
        #[doc(alias = "VIRTIO_NET_F_GUEST_CSUM")]
        const GUEST_CSUM = 1 << 1;

        /// Control channel offloads
        /// reconfiguration support.
        #[doc(alias = "VIRTIO_NET_F_CTRL_GUEST_OFFLOADS")]
        const CTRL_GUEST_OFFLOADS = 1 << 2;

        /// Device maximum MTU reporting is supported. If
        /// offered by the device, device advises driver about the value of
        /// its maximum MTU. If negotiated, the driver uses _mtu_ as
        /// the maximum MTU value.
        #[doc(alias = "VIRTIO_NET_F_MTU")]
        const MTU = 1 << 3;

        /// Device has given MAC address.
        #[doc(alias = "VIRTIO_NET_F_MAC")]
        const MAC = 1 << 5;

        /// Driver can receive TSOv4.
        #[doc(alias = "VIRTIO_NET_F_GUEST_TSO4")]
        const GUEST_TSO4 = 1 << 7;

        /// Driver can receive TSOv6.
        #[doc(alias = "VIRTIO_NET_F_GUEST_TSO6")]
        const GUEST_TSO6 = 1 << 8;

        /// Driver can receive TSO with ECN.
        #[doc(alias = "VIRTIO_NET_F_GUEST_ECN")]
        const GUEST_ECN = 1 << 9;

        /// Driver can receive UFO.
        #[doc(alias = "VIRTIO_NET_F_GUEST_UFO")]
        const GUEST_UFO = 1 << 10;

        /// Device can receive TSOv4.
        #[doc(alias = "VIRTIO_NET_F_HOST_TSO4")]
        const HOST_TSO4 = 1 << 11;

        /// Device can receive TSOv6.
        #[doc(alias = "VIRTIO_NET_F_HOST_TSO6")]
        const HOST_TSO6 = 1 << 12;

        /// Device can receive TSO with ECN.
        #[doc(alias = "VIRTIO_NET_F_HOST_ECN")]
        const HOST_ECN = 1 << 13;

        /// Device can receive UFO.
        #[doc(alias = "VIRTIO_NET_F_HOST_UFO")]
        const HOST_UFO = 1 << 14;

        /// Driver can merge receive buffers.
        #[doc(alias = "VIRTIO_NET_F_MRG_RXBUF")]
        const MRG_RXBUF = 1 << 15;

        /// Configuration status field is
        /// available.
        #[doc(alias = "VIRTIO_NET_F_STATUS")]
        const STATUS = 1 << 16;

        /// Control channel is available.
        #[doc(alias = "VIRTIO_NET_F_CTRL_VQ")]
        const CTRL_VQ = 1 << 17;

        /// Control channel RX mode support.
        #[doc(alias = "VIRTIO_NET_F_CTRL_RX")]
        const CTRL_RX = 1 << 18;

        /// Control channel VLAN filtering.
        #[doc(alias = "VIRTIO_NET_F_CTRL_VLAN")]
        const CTRL_VLAN = 1 << 19;

        /// Driver can send gratuitous
        /// packets.
        #[doc(alias = "VIRTIO_NET_F_GUEST_ANNOUNCE")]
        const GUEST_ANNOUNCE = 1 << 21;

        /// Device supports multiqueue with automatic
        /// receive steering.
        #[doc(alias = "VIRTIO_NET_F_MQ")]
        const MQ = 1 << 22;

        /// Set MAC address through control
        /// channel.
        #[doc(alias = "VIRTIO_NET_F_CTRL_MAC_ADDR")]
        const CTRL_MAC_ADDR = 1 << 23;

        /// Device can receive USO packets. Unlike UFO
        /// (fragmenting the packet) the USO splits large UDP packet
        /// to several segments when each of these smaller packets has UDP header.
        #[doc(alias = "VIRTIO_NET_F_HOST_USO")]
        const HOST_USO = 1 << 56;

        /// Device can report per-packet hash
        /// value and a type of calculated hash.
        #[doc(alias = "VIRTIO_NET_F_HASH_REPORT")]
        const HASH_REPORT = 1 << 57;

        /// Driver can provide the exact _hdr_len_
        /// value. Device benefits from knowing the exact header length.
        #[doc(alias = "VIRTIO_NET_F_GUEST_HDRLEN")]
        const GUEST_HDRLEN = 1 << 59;

        /// Device supports RSS (receive-side scaling)
        /// with Toeplitz hash calculation and configurable hash
        /// parameters for receive steering.
        #[doc(alias = "VIRTIO_NET_F_RSS")]
        const RSS = 1 << 60;

        /// Device can process duplicated ACKs
        /// and report number of coalesced segments and duplicated ACKs.
        #[doc(alias = "VIRTIO_NET_F_RSC_EXT")]
        const RSC_EXT = 1 << 61;

        /// Device may act as a standby for a primary
        /// device with the same MAC address.
        #[doc(alias = "VIRTIO_NET_F_STANDBY")]
        const STANDBY = 1 << 62;

        /// Device reports speed and duplex.
        #[doc(alias = "VIRTIO_NET_F_SPEED_DUPLEX")]
        const SPEED_DUPLEX = 1 << 63;

        const _ = !0;
    }
}
