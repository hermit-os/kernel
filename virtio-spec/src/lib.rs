//! This crate provides common definitions from the Virtio 1.2 specification.
//! This crate does not provide any additional driver-related functionality.
//!
//! For the actual specification see [Virtual I/O Device (VIRTIO) Version 1.2—Committee Specification 01](https://docs.oasis-open.org/virtio/virtio/v1.2/cs01/virtio-v1.2-cs01.html).

#![no_std]

#[macro_use]
mod bitflags;
#[macro_use]
pub mod volatile;
mod features;
pub mod mmio;
pub mod net;
pub mod pci;

pub use endian_num::{be128, be16, be32, be64, le128, le16, le32, le64};
use num_enum::{FromPrimitive, IntoPrimitive};

pub use self::features::{FeatureBits, F};

pub mod fs {
    //! File System Device

    pub use super::features::fs::F;
}

virtio_bitflags! {
    /// Device Status Field
    ///
    /// During device initialization by a driver,
    /// the driver follows the sequence of steps specified in
    /// _General Initialization And Device Operation / Device
    /// Initialization_.
    ///
    /// The `device status` field provides a simple low-level
    /// indication of the completed steps of this sequence.
    /// It's most useful to imagine it hooked up to traffic
    /// lights on the console indicating the status of each device.  The
    /// following bits are defined (listed below in the order in which
    /// they would be typically set):
    pub struct DeviceStatus: u8 {
        /// Indicates that the guest OS has found the
        /// device and recognized it as a valid virtio device.
        const ACKNOWLEDGE = 1;

        /// Indicates that the guest OS knows how to drive the
        /// device.
        ///
        /// <div class="warning">
        ///
        /// There could be a significant (or infinite) delay before setting
        /// this bit.  For example, under Linux, drivers can be loadable modules.
        ///
        /// </div>
        const DRIVER = 2;

        /// Indicates that something went wrong in the guest,
        /// and it has given up on the device. This could be an internal
        /// error, or the driver didn't like the device for some reason, or
        /// even a fatal error during device operation.
        const FAILED = 128;

        /// Indicates that the driver has acknowledged all the
        /// features it understands, and feature negotiation is complete.
        const FEATURES_OK = 8;

        /// Indicates that the driver is set up and ready to
        /// drive the device.
        const DRIVER_OK = 4;

        /// Indicates that the device has experienced
        /// an error from which it can't recover.
        const DEVICE_NEEDS_RESET = 64;
    }
}

/// Virtio Device IDs
#[derive(IntoPrimitive, FromPrimitive, PartialEq, Eq, Clone, Copy, Debug)]
#[non_exhaustive]
#[repr(u8)]
pub enum Id {
    /// reserved (invalid)
    Reserved = 0,

    /// network card
    Net = 1,

    /// block device
    Block = 2,

    /// console
    Console = 3,

    /// entropy source
    Rng = 4,

    /// memory ballooning (traditional)
    Balloon = 5,

    /// ioMemory
    IoMem = 6,

    /// rpmsg
    Rpmsg = 7,

    /// SCSI host
    Scsi = 8,

    /// 9P transport
    NineP = 9,

    /// mac80211 wlan
    Mac80211Wlan = 10,

    /// rproc serial
    RprocSerial = 11,

    /// virtio CAIF
    Caif = 12,

    /// memory balloon
    MemoryBalloon = 13,

    /// GPU device
    Gpu = 16,

    /// Timer/Clock device
    Clock = 17,

    /// Input device
    Input = 18,

    /// Socket device
    Vsock = 19,

    /// Crypto device
    Crypto = 20,

    /// Signal Distribution Module
    SignalDist = 21,

    /// pstore device
    Pstore = 22,

    /// IOMMU device
    Iommu = 23,

    /// Memory device
    Mem = 24,

    /// Audio device
    Sound = 25,

    /// file system device
    Fs = 26,

    /// PMEM device
    Pmem = 27,

    /// RPMB device
    Rpmb = 28,

    /// mac80211 hwsim wireless simulation device
    Mac80211Hwsim = 29,

    /// Video encoder device
    VideoEncoder = 30,

    /// Video decoder device
    VideoDecoder = 31,

    /// SCMI device
    Scmi = 32,

    /// NitroSecureModule
    NitroSecMod = 33,

    /// I2C adapter
    I2cAdapter = 34,

    /// Watchdog
    Watchdog = 35,

    /// CAN device
    Can = 36,

    /// Parameter Server
    ParamServ = 38,

    /// Audio policy device
    AudioPolicy = 39,

    /// Bluetooth device
    Bt = 40,

    /// GPIO device
    Gpio = 41,

    /// RDMA device
    Rdma = 42,

    /// Unknown device
    #[num_enum(catch_all)]
    Unknown(u8),
}
