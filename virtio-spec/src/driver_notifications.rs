use bitfield_struct::bitfield;

use crate::le32;

/// Notification Data.
#[bitfield(u32, repr = le32, from = le32::from_ne, into = le32::to_ne)]
pub struct NotificationData {
    /// VQ number to be notified.
    pub vqn: u16,

    /// Offset
    /// within the ring where the next available ring entry
    /// will be written.
    /// When [`VIRTIO_F_RING_PACKED`] has not been negotiated this refers to the
    /// 15 least significant bits of the available index.
    /// When `VIRTIO_F_RING_PACKED` has been negotiated this refers to the offset
    /// (in units of descriptor entries)
    /// within the descriptor ring where the next available
    /// descriptor will be written.
    ///
    /// [`VIRTIO_F_RING_PACKED`]: F::RING_PACKED
    #[bits(15)]
    pub next_off: u16,

    /// Wrap Counter.
    /// With [`VIRTIO_F_RING_PACKED`] this is the wrap counter
    /// referring to the next available descriptor.
    /// Without `VIRTIO_F_RING_PACKED` this is the most significant bit
    /// (bit 15) of the available index.
    ///
    /// [`VIRTIO_F_RING_PACKED`]: F::RING_PACKED
    #[bits(1)]
    pub next_wrap: u8,
}

impl NotificationData {
    const NEXT_IDX_BITS: usize = 16;
    const NEXT_IDX_OFFSET: usize = 16;

    /// Available index
    ///
    /// <div class="warning">
    ///
    /// This collides with [`Self::next_off`] and [`Self::next_wrap`].
    ///
    /// </div>
    ///
    /// Bits: 16..32
    pub const fn next_idx(&self) -> u16 {
        let mask = u32::MAX >> (u32::BITS - Self::NEXT_IDX_BITS as u32);
        let this = (le32::to_ne(self.0) >> Self::NEXT_IDX_OFFSET) & mask;
        this as u16
    }

    /// Available index
    ///
    /// <div class="warning">
    ///
    /// This collides with [`Self::with_next_off`] and [`Self::with_next_wrap`].
    ///
    /// </div>
    ///
    /// Bits: 16..32
    pub const fn with_next_idx(self, value: u16) -> Self {
        let mask = u32::MAX >> (u32::BITS - Self::NEXT_IDX_BITS as u32);
        let bits = le32::to_ne(self.0) & !(mask << Self::NEXT_IDX_OFFSET)
            | (value as u32 & mask) << Self::NEXT_IDX_OFFSET;
        Self(le32::from_ne(bits))
    }

    /// Available index
    ///
    /// <div class="warning">
    ///
    /// This collides with [`Self::set_next_off`] and [`Self::set_next_wrap`].
    ///
    /// </div>
    ///
    /// Bits: 16..32
    pub fn set_next_idx(&mut self, value: u16) {
        *self = self.with_next_idx(value);
    }
}
