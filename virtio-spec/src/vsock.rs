//! Socket Device

use endian_num::{le16, le32};
use num_enum::{IntoPrimitive, TryFromPrimitive, UnsafeFromPrimitive};
use volatile_macro::VolatileFieldAccess;

use crate::le64;

/// Socket Device Configuration Layout
#[doc(alias = "virtio_vsock_config")]
#[cfg_attr(
    feature = "zerocopy",
    derive(zerocopy_derive::FromZeroes, zerocopy_derive::FromBytes)
)]
#[derive(VolatileFieldAccess)]
#[repr(C)]
pub struct Config {
    guest_cid: le64,
}

/// Socket Device Header
#[doc(alias = "virtio_vsock_hdr")]
#[cfg_attr(
    feature = "zerocopy",
    derive(
        zerocopy_derive::FromZeroes,
        zerocopy_derive::FromBytes,
        zerocopy_derive::AsBytes
    )
)]
#[derive(Default, Clone, Copy, Debug)]
#[repr(C, packed)]
pub struct Hdr {
    pub src_cid: le64,
    pub dst_cid: le64,
    pub src_port: le32,
    pub dst_port: le32,
    pub len: le32,
    pub type_: le16,
    pub op: le16,
    pub flags: le32,
    pub buf_alloc: le32,
    pub fwd_cnt: le32,
}

#[doc(alias = "VIRTIO_VSOCK_OP")]
#[derive(
    IntoPrimitive, TryFromPrimitive, UnsafeFromPrimitive, PartialEq, Eq, Clone, Copy, Debug,
)]
#[non_exhaustive]
#[repr(u16)]
pub enum Op {
    #[doc(alias = "VIRTIO_VSOCK_OP_INVALID")]
    Invalid = 0,

    #[doc(alias = "VIRTIO_VSOCK_OP_REQUEST")]
    Request = 1,

    #[doc(alias = "VIRTIO_VSOCK_OP_RESPONSE")]
    Response = 2,

    #[doc(alias = "VIRTIO_VSOCK_OP_RST")]
    Rst = 3,

    #[doc(alias = "VIRTIO_VSOCK_OP_SHUTDOWN")]
    Shutdown = 4,

    #[doc(alias = "VIRTIO_VSOCK_OP_RW")]
    Rw = 5,

    #[doc(alias = "VIRTIO_VSOCK_OP_CREDIT_UPDATE")]
    CreditUpdate = 6,

    #[doc(alias = "VIRTIO_VSOCK_OP_CREDIT_REQUEST")]
    CreditRequest = 7,
}

#[doc(alias = "VIRTIO_VSOCK_TYPE")]
#[derive(
    IntoPrimitive, TryFromPrimitive, UnsafeFromPrimitive, PartialEq, Eq, Clone, Copy, Debug,
)]
#[non_exhaustive]
#[repr(u16)]
pub enum Type {
    #[doc(alias = "VIRTIO_VSOCK_TYPE_STREAM")]
    Stream = 1,

    #[doc(alias = "VIRTIO_VSOCK_TYPE_SEQPACKET")]
    Seqpacket = 2,
}

endian_bitflags! {
    /// Socket Device Shutdown Flags
    #[doc(alias = "VIRTIO_VSOCK_SHUTDOWN_F")]
    pub struct ShutdownF: le32 {
        #[doc(alias = "VIRTIO_VSOCK_SHUTDOWN_F_RECEIVE")]
        const RECEIVE = 1 << 0;

        #[doc(alias = "VIRTIO_VSOCK_SHUTDOWN_F_SEND")]
        const SEND = 1 << 1;
    }
}

endian_bitflags! {
    /// Socket Device Sequence Flags
    #[doc(alias = "VIRTIO_VSOCK_SEQ")]
    pub struct Seq: le32 {
        #[doc(alias = "VIRTIO_VSOCK_SEQ_EOM")]
        const EOM = 1 << 0;

        #[doc(alias = "VIRTIO_VSOCK_SEQ_EOR")]
        const EOR = 1 << 1;
    }
}

#[doc(alias = "VIRTIO_VSOCK_EVENT")]
#[derive(
    IntoPrimitive, TryFromPrimitive, UnsafeFromPrimitive, PartialEq, Eq, Clone, Copy, Debug,
)]
#[non_exhaustive]
#[repr(u32)]
pub enum EventId {
    #[doc(alias = "VIRTIO_VSOCK_EVENT_TRANSPORT_RESET")]
    TransportReset = 0,
}

/// Socket Device Event
#[doc(alias = "virtio_vsock_event")]
#[cfg_attr(
    feature = "zerocopy",
    derive(
        zerocopy_derive::FromZeroes,
        zerocopy_derive::FromBytes,
        zerocopy_derive::AsBytes
    )
)]
#[derive(Default, Clone, Copy, Debug)]
#[repr(C)]
pub struct Event {
    id: le32,
}
