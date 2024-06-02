//! Definitions for Virtio over MMIO.

use core::mem;

use endian_num::{le16, le32};
use volatile::access::{ReadOnly, ReadWrite, RestrictAccess, WriteOnly};
use volatile::VolatilePtr;

use crate::volatile::{OveralignedVolatilePtr, WideVolatilePtr};
use crate::{DeviceStatus, Id};

/// MMIO Device Registers
#[repr(transparent)]
pub struct DeviceRegisters([le32; 0x100 / mem::size_of::<le32>()]);

macro_rules! field_fn {
    (
        $(#[doc = $doc:literal])*
        #[doc(alias = $alias:literal)]
        #[access($Access:ty)]
        $field:ident: le32,
    ) => {
        $(#[doc = $doc])*
        #[doc(alias = $alias)]
        fn $field(self) -> VolatilePtr<'a, le32, A::Restricted>
        where
            A: RestrictAccess<$Access>;
    };
    (
        $(#[doc = $doc:literal])*
        #[doc(alias = $alias:literal)]
        #[access($Access:ty)]
        $field:ident: $T:ty,
    ) => {
        $(#[doc = $doc])*
        #[doc(alias = $alias)]
        fn $field(self) -> OveralignedVolatilePtr<'a, $T, le32, A::Restricted>
        where
            A: RestrictAccess<$Access>;
    };
}

macro_rules! field_impl {
    (
        #[offset($offset:literal)]
        #[access($Access:ty)]
        $field:ident: le32,
    ) => {
        fn $field(self) -> VolatilePtr<'a, le32, A::Restricted>
        where
            A: RestrictAccess<$Access>,
        {
            unsafe {
                self.map(|ptr| ptr.cast::<le32>().byte_add($offset))
                    .restrict()
            }
        }
    };
    (
        #[offset($offset:literal)]
        #[access($Access:ty)]
        $field:ident: $T:ty,
    ) => {
        fn $field(self) -> OveralignedVolatilePtr<'a, $T, le32, A::Restricted>
        where
            A: RestrictAccess<$Access>,
        {
            let ptr = unsafe { self.map(|ptr| ptr.cast::<le32>().byte_add($offset)) };
            OveralignedVolatilePtr::new(ptr.restrict())
        }
    };
}

macro_rules! device_register_impl {
    (
        $(#[doc = $outer_doc:literal])*
        pub struct DeviceRegisters {
            $(
                $(#[doc = $doc:literal])*
                #[doc(alias = $alias:literal)]
                #[offset($offset:literal)]
                #[access($Access:ty)]
                $field:ident: $T:ident,
            )*
        }
    ) => {
        $(#[doc = $outer_doc])*
        pub trait DeviceRegisterVolatileFieldAccess<'a, A> {
            $(
                field_fn! {
                    $(#[doc = $doc])*
                    #[doc(alias = $alias)]
                    #[access($Access)]
                    $field: $T,
                }
            )*
        }

        impl<'a, A> DeviceRegisterVolatileFieldAccess<'a, A> for VolatilePtr<'a, DeviceRegisters, A> {
            $(
                field_impl! {
                    #[offset($offset)]
                    #[access($Access)]
                    $field: $T,
                }
            )*
        }
    };
}

device_register_impl! {
    /// MMIO Device Registers
    pub struct DeviceRegisters {
        /// Magic Value
        ///
        /// 0x74726976
        /// (a Little Endian equivalent of the “virt” string).
        #[doc(alias = "MagicValue")]
        #[offset(0x000)]
        #[access(ReadOnly)]
        magic_value: le32,

        /// Device version number
        ///
        /// 0x2.
        ///
        /// <div class="warning">
        ///
        /// Legacy devices (see _Virtio Transport Options / Virtio Over MMIO / Legacy interface_) used 0x1.
        ///
        /// </div>
        #[doc(alias = "Version")]
        #[offset(0x004)]
        #[access(ReadOnly)]
        version: le32,

        /// Virtio Subsystem Device ID
        ///
        /// See _Device Types_ for possible values.
        /// Value zero (0x0) is used to
        /// define a system memory map with placeholder devices at static,
        /// well known addresses, assigning functions to them depending
        /// on user's needs.
        #[doc(alias = "DeviceID")]
        #[offset(0x008)]
        #[access(ReadOnly)]
        device_id: Id,

        /// Virtio Subsystem Vendor ID
        #[doc(alias = "VendorID")]
        #[offset(0x00c)]
        #[access(ReadOnly)]
        vendor_id: le32,

        /// Flags representing features the device supports
        ///
        /// Reading from this register returns 32 consecutive flag bits,
        /// the least significant bit depending on the last value written to
        /// `DeviceFeaturesSel`. Access to this register returns
        /// bits `DeviceFeaturesSel`*32 to (`DeviceFeaturesSel`*32)+31, eg.
        /// feature bits 0 to 31 if `DeviceFeaturesSel` is set to 0 and
        /// features bits 32 to 63 if `DeviceFeaturesSel` is set to 1.
        /// Also see _Basic Facilities of a Virtio Device / Feature Bits_.
        #[doc(alias = "DeviceFeatures")]
        #[offset(0x010)]
        #[access(ReadOnly)]
        device_features: le32,

        /// Device (host) features word selection.
        ///
        /// Writing to this register selects a set of 32 device feature bits
        /// accessible by reading from `DeviceFeatures`.
        #[doc(alias = "DeviceFeaturesSel")]
        #[offset(0x014)]
        #[access(WriteOnly)]
        device_features_sel: le32,

        /// Flags representing device features understood and activated by the driver
        ///
        /// Writing to this register sets 32 consecutive flag bits, the least significant
        /// bit depending on the last value written to `DriverFeaturesSel`.
        ///  Access to this register sets bits `DriverFeaturesSel`*32
        /// to (`DriverFeaturesSel`*32)+31, eg. feature bits 0 to 31 if
        /// `DriverFeaturesSel` is set to 0 and features bits 32 to 63 if
        /// `DriverFeaturesSel` is set to 1. Also see _Basic Facilities of a Virtio Device / Feature Bits_.
        #[doc(alias = "DriverFeatures")]
        #[offset(0x020)]
        #[access(WriteOnly)]
        driver_features: le32,

        /// Activated (guest) features word selection
        ///
        /// Writing to this register selects a set of 32 activated feature
        /// bits accessible by writing to `DriverFeatures`.
        #[doc(alias = "DriverFeaturesSel")]
        #[offset(0x024)]
        #[access(WriteOnly)]
        driver_features_sel: le32,

        /// Virtual queue index
        ///
        /// Writing to this register selects the virtual queue that the
        /// following operations on `QueueNumMax`, `QueueNum`, `QueueReady`,
        /// `QueueDescLow`, `QueueDescHigh`, `QueueDriverlLow`, `QueueDriverHigh`,
        /// `QueueDeviceLow`, `QueueDeviceHigh` and `QueueReset` apply to. The index
        /// number of the first queue is zero (0x0).
        #[doc(alias = "QueueSel")]
        #[offset(0x030)]
        #[access(WriteOnly)]
        queue_sel: le16,

        /// Maximum virtual queue size
        ///
        /// Reading from the register returns the maximum size (number of
        /// elements) of the queue the device is ready to process or
        /// zero (0x0) if the queue is not available. This applies to the
        /// queue selected by writing to `QueueSel`.
        #[doc(alias = "QueueNumMax")]
        #[offset(0x034)]
        #[access(ReadOnly)]
        queue_num_max: le16,

        /// Virtual queue size
        ///
        /// Queue size is the number of elements in the queue.
        /// Writing to this register notifies the device what size of the
        /// queue the driver will use. This applies to the queue selected by
        /// writing to `QueueSel`.
        #[doc(alias = "QueueNum")]
        #[offset(0x038)]
        #[access(WriteOnly)]
        queue_num: le16,

        /// Virtual queue ready bit
        ///
        /// Writing one (0x1) to this register notifies the device that it can
        /// execute requests from this virtual queue. Reading from this register
        /// returns the last value written to it. Both read and write
        /// accesses apply to the queue selected by writing to `QueueSel`.
        #[doc(alias = "QueueReady")]
        #[offset(0x044)]
        #[access(ReadWrite)]
        queue_ready: bool,

        /// Queue notifier
        ///
        /// Writing a value to this register notifies the device that
        /// there are new buffers to process in a queue.
        ///
        /// When VIRTIO_F_NOTIFICATION_DATA has not been negotiated,
        /// the value written is the queue index.
        ///
        /// When VIRTIO_F_NOTIFICATION_DATA has been negotiated,
        /// the `Notification data` value has the following format:
        ///
        /// ```c
        /// le32 {
        ///   vqn : 16;
        ///   next_off : 15;
        ///   next_wrap : 1;
        /// };
        /// ```
        ///
        /// See _Virtqueues / Driver notifications_
        /// for the definition of the components.
        #[doc(alias = "QueueNotify")]
        #[offset(0x050)]
        #[access(WriteOnly)]
        queue_notify: le32,

        /// Interrupt status
        ///
        /// Reading from this register returns a bit mask of events that
        /// caused the device interrupt to be asserted.
        #[doc(alias = "InterruptStatus")]
        #[offset(0x060)]
        #[access(ReadOnly)]
        interrupt_status: InterruptStatus,

        /// Interrupt acknowledge
        ///
        /// Writing a value with bits set as defined in `InterruptStatus`
        /// to this register notifies the device that events causing
        /// the interrupt have been handled.
        #[doc(alias = "InterruptACK")]
        #[offset(0x064)]
        #[access(WriteOnly)]
        interrupt_ack: InterruptStatus,

        /// Device status
        ///
        /// Reading from this register returns the current device status
        /// flags.
        /// Writing non-zero values to this register sets the status flags,
        /// indicating the driver progress. Writing zero (0x0) to this
        /// register triggers a device reset.
        /// See also p. _Virtio Transport Options / Virtio Over MMIO / MMIO-specific Initialization And Device Operation / Device Initialization_.
        #[doc(alias = "Status")]
        #[offset(0x070)]
        #[access(ReadWrite)]
        status: DeviceStatus,

        /// Virtual queue's Descriptor Area 64 bit long physical address
        ///
        /// Writing to these two registers (lower 32 bits of the address
        /// to `QueueDescLow`, higher 32 bits to `QueueDescHigh`) notifies
        /// the device about location of the Descriptor Area of the queue
        /// selected by writing to `QueueSel` register.
        #[doc(alias = "QueueDescLow")]
        #[offset(0x080)]
        #[access(WriteOnly)]
        queue_desc_low: le32,

        /// Virtual queue's Descriptor Area 64 bit long physical address
        ///
        /// Writing to these two registers (lower 32 bits of the address
        /// to `QueueDescLow`, higher 32 bits to `QueueDescHigh`) notifies
        /// the device about location of the Descriptor Area of the queue
        /// selected by writing to `QueueSel` register.
        #[doc(alias = "QueueDescHigh")]
        #[offset(0x084)]
        #[access(WriteOnly)]
        queue_desc_high: le32,

        /// Virtual queue's Driver Area 64 bit long physical address
        ///
        /// Writing to these two registers (lower 32 bits of the address
        /// to `QueueDriverLow`, higher 32 bits to `QueueDriverHigh`) notifies
        /// the device about location of the Driver Area of the queue
        /// selected by writing to `QueueSel`.
        #[doc(alias = "QueueDriverLow")]
        #[offset(0x090)]
        #[access(WriteOnly)]
        queue_driver_low: le32,

        /// Virtual queue's Driver Area 64 bit long physical address
        ///
        /// Writing to these two registers (lower 32 bits of the address
        /// to `QueueDriverLow`, higher 32 bits to `QueueDriverHigh`) notifies
        /// the device about location of the Driver Area of the queue
        /// selected by writing to `QueueSel`.
        #[doc(alias = "QueueDriverHigh")]
        #[offset(0x094)]
        #[access(WriteOnly)]
        queue_driver_high: le32,

        /// Virtual queue's Device Area 64 bit long physical address
        ///
        /// Writing to these two registers (lower 32 bits of the address
        /// to `QueueDeviceLow`, higher 32 bits to `QueueDeviceHigh`) notifies
        /// the device about location of the Device Area of the queue
        /// selected by writing to `QueueSel`.
        #[doc(alias = "QueueDeviceLow")]
        #[offset(0x0a0)]
        #[access(WriteOnly)]
        queue_device_low: le32,

        /// Virtual queue's Device Area 64 bit long physical address
        ///
        /// Writing to these two registers (lower 32 bits of the address
        /// to `QueueDeviceLow`, higher 32 bits to `QueueDeviceHigh`) notifies
        /// the device about location of the Device Area of the queue
        /// selected by writing to `QueueSel`.
        #[doc(alias = "QueueDeviceHigh")]
        #[offset(0x0a4)]
        #[access(WriteOnly)]
        queue_device_high: le32,

        /// Shared memory id
        ///
        /// Writing to this register selects the shared memory region _Basic Facilities of a Virtio Device / Shared Memory Regions_
        /// following operations on `SHMLenLow`, `SHMLenHigh`,
        /// `SHMBaseLow` and `SHMBaseHigh` apply to.
        #[doc(alias = "SHMSel")]
        #[offset(0x0ac)]
        #[access(WriteOnly)]
        shm_sel: le32,

        /// Shared memory region 64 bit long length
        ///
        /// These registers return the length of the shared memory
        /// region in bytes, as defined by the device for the region selected by
        /// the `SHMSel` register.  The lower 32 bits of the length
        /// are read from `SHMLenLow` and the higher 32 bits from
        /// `SHMLenHigh`.  Reading from a non-existent
        /// region (i.e. where the ID written to `SHMSel` is unused)
        /// results in a length of -1.
        #[doc(alias = "SHMLenLow")]
        #[offset(0x0b0)]
        #[access(ReadOnly)]
        shm_len_low: le32,

        /// Shared memory region 64 bit long length
        ///
        /// These registers return the length of the shared memory
        /// region in bytes, as defined by the device for the region selected by
        /// the `SHMSel` register.  The lower 32 bits of the length
        /// are read from `SHMLenLow` and the higher 32 bits from
        /// `SHMLenHigh`.  Reading from a non-existent
        /// region (i.e. where the ID written to `SHMSel` is unused)
        /// results in a length of -1.
        #[doc(alias = "SHMLenHigh")]
        #[offset(0x0b4)]
        #[access(ReadOnly)]
        shm_len_high: le32,

        /// Shared memory region 64 bit long physical address
        ///
        /// The driver reads these registers to discover the base address
        /// of the region in physical address space.  This address is
        /// chosen by the device (or other part of the VMM).
        /// The lower 32 bits of the address are read from `SHMBaseLow`
        /// with the higher 32 bits from `SHMBaseHigh`.  Reading
        /// from a non-existent region (i.e. where the ID written to
        /// `SHMSel` is unused) results in a base address of
        /// 0xffffffffffffffff.
        #[doc(alias = "SHMBaseLow")]
        #[offset(0x0b8)]
        #[access(ReadOnly)]
        shm_base_low: le32,

        /// Shared memory region 64 bit long physical address
        ///
        /// The driver reads these registers to discover the base address
        /// of the region in physical address space.  This address is
        /// chosen by the device (or other part of the VMM).
        /// The lower 32 bits of the address are read from `SHMBaseLow`
        /// with the higher 32 bits from `SHMBaseHigh`.  Reading
        /// from a non-existent region (i.e. where the ID written to
        /// `SHMSel` is unused) results in a base address of
        /// 0xffffffffffffffff.
        #[doc(alias = "SHMBaseHigh")]
        #[offset(0x0bc)]
        #[access(ReadOnly)]
        shm_base_high: le32,

        /// Virtual queue reset bit
        ///
        /// If VIRTIO_F_RING_RESET has been negotiated, writing one (0x1) to this
        /// register selectively resets the queue. Both read and write accesses
        /// apply to the queue selected by writing to `QueueSel`.
        #[doc(alias = "QueueReset")]
        #[offset(0x0c0)]
        #[access(ReadWrite)]
        queue_reset: le32,

        /// Configuration atomicity value
        ///
        /// Reading from this register returns a value describing a version of the device-specific configuration space (see `Config`).
        /// The driver can then access the configuration space and, when finished, read `ConfigGeneration` again.
        /// If no part of the configuration space has changed between these two `ConfigGeneration` reads, the returned values are identical.
        /// If the values are different, the configuration space accesses were not atomic and the driver has to perform the operations again.
        /// See also _Basic Facilities of a Virtio Device / Device Configuration Space_.
        #[doc(alias = "ConfigGeneration")]
        #[offset(0x0fc)]
        #[access(ReadOnly)]
        config_generation: le32,
    }
}

impl_wide_field_access! {
    /// MMIO Device Registers
    pub trait DeviceRegisterVolatileWideFieldAccess<'a, A>: DeviceRegisters {
        /// Virtual queue's Descriptor Area 64 bit long physical address
        ///
        /// Writing to these two registers (lower 32 bits of the address
        /// to `QueueDescLow`, higher 32 bits to `QueueDescHigh`) notifies
        /// the device about location of the Descriptor Area of the queue
        /// selected by writing to `QueueSel` register.
        #[doc(alias = "QueueDesc")]
        #[access(WriteOnly)]
        queue_desc: queue_desc_low, queue_desc_high;

        /// Virtual queue's Driver Area 64 bit long physical address
        ///
        /// Writing to these two registers (lower 32 bits of the address
        /// to `QueueDriverLow`, higher 32 bits to `QueueDriverHigh`) notifies
        /// the device about location of the Driver Area of the queue
        /// selected by writing to `QueueSel`.
        #[doc(alias = "QueueDriver")]
        #[access(WriteOnly)]
        queue_driver: queue_driver_low, queue_driver_high;

        /// Virtual queue's Device Area 64 bit long physical address
        ///
        /// Writing to these two registers (lower 32 bits of the address
        /// to `QueueDeviceLow`, higher 32 bits to `QueueDeviceHigh`) notifies
        /// the device about location of the Device Area of the queue
        /// selected by writing to `QueueSel`.
        #[doc(alias = "QueueDevice")]
        #[access(WriteOnly)]
        queue_device: queue_device_low, queue_device_high;

        /// Shared memory region 64 bit long length
        ///
        /// These registers return the length of the shared memory
        /// region in bytes, as defined by the device for the region selected by
        /// the `SHMSel` register.  The lower 32 bits of the length
        /// are read from `SHMLenLow` and the higher 32 bits from
        /// `SHMLenHigh`.  Reading from a non-existent
        /// region (i.e. where the ID written to `SHMSel` is unused)
        /// results in a length of -1.
        #[doc(alias = "SHMLen")]
        #[access(ReadOnly)]
        shm_len: shm_len_low, shm_len_high;

        /// Shared memory region 64 bit long physical address
        ///
        /// The driver reads these registers to discover the base address
        /// of the region in physical address space.  This address is
        /// chosen by the device (or other part of the VMM).
        /// The lower 32 bits of the address are read from `SHMBaseLow`
        /// with the higher 32 bits from `SHMBaseHigh`.  Reading
        /// from a non-existent region (i.e. where the ID written to
        /// `SHMSel` is unused) results in a base address of
        /// 0xffffffffffffffff.
        #[doc(alias = "SHMBase")]
        #[access(ReadOnly)]
        shm_base: shm_base_low, shm_base_high;
    }
}

virtio_bitflags! {
    /// Interrupt Status
    pub struct InterruptStatus: u8 {
        /// Used Buffer Notification
        ///
        /// The interrupt was asserted because the device has used a buffer in at least one of the active virtual queues.
        const USED_BUFFER_NOTIFICATION = 1 << 0;

        /// Configuration Change Notification
        ///
        /// The interrupt was asserted because the configuration of the device has changed.
        const CONFIGURATION_CHANGE_NOTIFICATION = 1 << 1;
    }
}
