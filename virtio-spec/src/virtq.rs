//! Virtqueue definitions

use endian_num::le16;

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
