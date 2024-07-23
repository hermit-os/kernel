//! Packed virtqueue definitions

use bitfield_struct::bitfield;

use crate::{le16, le32, le64, virtq, RingEventFlags};

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

/// Event Suppression Descriptor
#[doc(alias = "pvirtq_event_suppress")]
#[repr(C)]
pub struct EventSuppress {
    /// If desc_event_flags set to RING_EVENT_FLAGS_DESC
    pub desc: EventSuppressDesc,
    pub flags: EventSuppressFlags,
}

/// Event Suppression Flags
#[bitfield(u16, repr = le16, from = le16::from_ne, into = le16::to_ne)]
pub struct EventSuppressDesc {
    /// Descriptor Ring Change Event Offset
    #[bits(15)]
    pub desc_event_off: u16,

    /// Descriptor Ring Change Event Wrap Counter
    #[bits(1)]
    pub desc_event_wrap: u8,
}

#[bitfield(u16, repr = le16, from = le16::from_ne, into = le16::to_ne)]
pub struct EventSuppressFlags {
    /// Descriptor Ring Change Event Flags
    #[bits(2)]
    pub desc_event_flags: RingEventFlags,

    /// Reserved, set to 0
    #[bits(14)]
    pub reserved: u16,
}
