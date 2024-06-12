use endian_num::{le16, le32, le64};

use crate::virtq;

/// Packed Virtqueue Descriptor
#[doc(alias = "pvirtq_desc")]
#[repr(C)]
pub struct Desc {
    /// Buffer Address.
    pub addr: le64,

    /// Buffer Length.
    pub len: le32,

    /// Buffer ID.
    pub id: le16,

    /// The flags depending on descriptor type.
    pub flags: virtq::DescF,
}
