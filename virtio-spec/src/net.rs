//! Network Device

pub use super::features::net::F;
use crate::{le16, le32};

virtio_bitflags! {
    /// Network Device Header Flags
    #[doc(alias = "VIRTIO_NET_HDR_F")]
    pub struct HdrF: u8 {
        #[doc(alias = "VIRTIO_NET_HDR_F_NEEDS_CSUM")]
        const NEEDS_CSUM = 1;

        #[doc(alias = "VIRTIO_NET_HDR_F_DATA_VALID")]
        const DATA_VALID = 2;

        #[doc(alias = "VIRTIO_NET_HDR_F_RSC_INFO")]
        const RSC_INFO = 4;
    }
}

virtio_bitflags! {
    /// Network Device Header GSO Type
    #[doc(alias = "VIRTIO_NET_HDR_GSO")]
    pub struct HdrGso: u8 {
        #[doc(alias = "VIRTIO_NET_HDR_GSO_NONE")]
        const NONE = 0;

        #[doc(alias = "VIRTIO_NET_HDR_GSO_TCPV4")]
        const TCPV4 = 1;

        #[doc(alias = "VIRTIO_NET_HDR_GSO_UDP")]
        const UDP = 3;

        #[doc(alias = "VIRTIO_NET_HDR_GSO_TCPV6")]
        const TCPV6 = 4;

        #[doc(alias = "VIRTIO_NET_HDR_GSO_UDP_L4")]
        const UDP_L4 = 5;

        #[doc(alias = "VIRTIO_NET_HDR_GSO_ECN")]
        const ECN = 0x80;
    }
}

/// Network Device Header
#[doc(alias = "virtio_net_hdr")]
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
pub struct Hdr {
    pub flags: HdrF,
    pub gso_type: HdrGso,
    pub hdr_len: le16,
    pub gso_size: le16,
    pub csum_start: le16,
    pub csum_offset: le16,
    pub num_buffers: le16,
}

/// Network Device Header Hash Report
///
/// Only if VIRTIO_NET_F_HASH_REPORT negotiated
#[doc(alias = "virtio_net_hdr")]
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
pub struct HdrHashReport {
    /// Only if VIRTIO_NET_F_HASH_REPORT negotiated
    pub hash_value: le32,
    /// Only if VIRTIO_NET_F_HASH_REPORT negotiated
    pub hash_report: le16,
    /// Only if VIRTIO_NET_F_HASH_REPORT negotiated
    pub padding_reserved: le16,
}
