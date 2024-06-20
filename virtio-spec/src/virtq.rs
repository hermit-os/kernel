//! Virtqueue definitions

use endian_num::{le16, le32, le64};

/// Split Virtqueue Descriptor
#[doc(alias = "virtq_desc")]
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct Desc {
    /// Address (guest-physical).
    pub addr: le64,

    /// Length.
    pub len: le32,

    /// The flags as indicated in [`DescF`].
    pub flags: DescF,

    /// Next field if flags & NEXT
    pub next: le16,
}

endian_bitflags! {
    /// Virtqueue descriptor flags
    #[doc(alias = "VIRTQ_DESC_F")]
    pub struct DescF: le16 {
        /// This marks a buffer as continuing via the next field.
        #[doc(alias = "VIRTQ_DESC_F_NEXT")]
        const NEXT = 1;

        /// This marks a buffer as device write-only (otherwise device read-only).
        #[doc(alias = "VIRTQ_DESC_F_WRITE")]
        const WRITE = 2;

        /// This means the buffer contains a list of buffer descriptors.
        #[doc(alias = "VIRTQ_DESC_F_INDIRECT")]
        const INDIRECT = 4;

        #[doc(alias = "VIRTQ_DESC_F_AVAIL")]
        const AVAIL = 1 << 7;

        #[doc(alias = "VIRTQ_DESC_F_USED")]
        const USED = 1 << 15;
    }
}

/// Used Ring Entry
#[doc(alias = "virtq_used_elem")]
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct UsedElem {
    /// Index of start of used descriptor chain.
    ///
    /// le32 is used here for ids for padding reasons.
    pub id: le32,

    /// The number of bytes written into the device writable portion of
    /// the buffer described by the descriptor chain.
    pub len: le32,
}
