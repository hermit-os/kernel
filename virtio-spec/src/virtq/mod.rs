//! Virtqueue definitions

#[cfg(feature = "alloc")]
mod alloc;

use core::alloc::Layout;
use core::ptr::{addr_of_mut, NonNull};
use core::{mem, ptr};

use crate::{le16, le32, le64};

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

/// The Virtqueue Available Ring
#[doc(alias = "virtq_avail")]
#[derive(Debug)]
#[repr(C)]
pub struct Avail {
    pub flags: AvailF,
    pub idx: le16,
    ring_and_used_event: [le16],
}

impl Avail {
    pub fn layout(queue_size: u16, has_event_idx: bool) -> Layout {
        Layout::array::<le16>(2 + usize::from(queue_size) + usize::from(has_event_idx)).unwrap()
    }

    pub fn from_ptr(ptr: NonNull<[u8]>) -> Option<NonNull<Self>> {
        let len = ptr.as_ptr().len();
        // FIXME: use ptr::as_mut_ptr once stable
        // https://github.com/rust-lang/rust/issues/74265
        let ptr = ptr.as_ptr() as *mut u8;

        if !ptr.cast::<le16>().is_aligned() {
            return None;
        }

        if len % mem::size_of::<le16>() != 0 {
            return None;
        }

        let len = len / mem::size_of::<le16>() - 2;
        let ptr = ptr::slice_from_raw_parts_mut(ptr, len) as *mut Self;
        Some(NonNull::new(ptr).unwrap())
    }

    pub fn ring_ptr(this: NonNull<Self>, has_event_idx: bool) -> NonNull<[le16]> {
        let ptr = unsafe { addr_of_mut!((*this.as_ptr()).ring_and_used_event) };
        let len = if cfg!(debug_assertions) {
            ptr.len()
                .checked_sub(usize::from(has_event_idx))
                .expect("`has_event_idx` cannot be true if it was not true at creation")
        } else {
            ptr.len().saturating_sub(usize::from(has_event_idx))
        };
        let ptr = NonNull::new(ptr).unwrap().cast::<le16>();
        NonNull::slice_from_raw_parts(ptr, len)
    }

    pub fn ring(&self, has_event_idx: bool) -> &[le16] {
        let ptr = Self::ring_ptr(NonNull::from(self), has_event_idx);
        unsafe { ptr.as_ref() }
    }

    pub fn ring_mut(&mut self, has_event_idx: bool) -> &mut [le16] {
        let mut ptr = Self::ring_ptr(NonNull::from(self), has_event_idx);
        unsafe { ptr.as_mut() }
    }

    pub fn used_event_ptr(this: NonNull<Self>, has_event_idx: bool) -> Option<NonNull<le16>> {
        if !has_event_idx {
            return None;
        }

        let ptr = unsafe { addr_of_mut!((*this.as_ptr()).ring_and_used_event) };
        let len = ptr.len();

        if len == 0 {
            return None;
        }

        let ptr = NonNull::new(ptr).unwrap().cast::<le16>();
        let ptr = unsafe { ptr.add(len - 1) };
        Some(ptr)
    }

    pub fn used_event(&self, has_event_idx: bool) -> Option<&le16> {
        Self::used_event_ptr(NonNull::from(self), has_event_idx).map(|ptr| unsafe { ptr.as_ref() })
    }

    pub fn used_event_mut(&mut self, has_event_idx: bool) -> Option<&mut le16> {
        Self::used_event_ptr(NonNull::from(self), has_event_idx)
            .map(|mut ptr| unsafe { ptr.as_mut() })
    }
}

endian_bitflags! {
    /// Virtqueue available ring flags
    #[doc(alias = "VIRTQ_AVAIL_F")]
    pub struct AvailF: le16 {
        /// The driver uses this in avail->flags to advise the device: don’t
        /// interrupt me when you consume a buffer.  It’s unreliable, so it’s
        /// simply an optimization.
        #[doc(alias = "VIRTQ_AVAIL_F_NO_INTERRUPT")]
        const NO_INTERRUPT = 1;
    }
}

/// The Virtqueue Used Ring
#[doc(alias = "virtq_used")]
#[derive(Debug)]
#[repr(C)]
#[repr(align(4))] // mem::align_of::<UsedElem>
pub struct Used {
    pub flags: UsedF,
    pub idx: le16,
    ring_and_avail_event: [le16],
}

impl Used {
    pub fn layout(queue_size: u16, has_event_idx: bool) -> Layout {
        let event_idx_layout = if has_event_idx {
            Layout::new::<le16>()
        } else {
            Layout::new::<()>()
        };

        Layout::array::<le16>(2)
            .unwrap()
            .extend(Layout::array::<UsedElem>(queue_size.into()).unwrap())
            .unwrap()
            .0
            .extend(event_idx_layout)
            .unwrap()
            .0
            .pad_to_align()
    }

    pub fn from_ptr(ptr: NonNull<[u8]>, has_event_idx: bool) -> Option<NonNull<Self>> {
        let len = ptr.len();
        let ptr = ptr.cast::<u8>().as_ptr();

        if !ptr.cast::<UsedElem>().is_aligned() {
            return None;
        }

        if len % mem::size_of::<UsedElem>() != usize::from(!has_event_idx) * mem::size_of::<le32>()
        {
            return None;
        }

        let len = len / mem::size_of::<le16>() - 2 - usize::from(has_event_idx);
        let ptr = ptr::slice_from_raw_parts(ptr, len) as *mut Used;
        Some(NonNull::new(ptr).unwrap())
    }

    pub fn ring_ptr(this: NonNull<Self>) -> NonNull<[UsedElem]> {
        let ptr = unsafe { addr_of_mut!((*this.as_ptr()).ring_and_avail_event) };
        let len = ptr.len() * mem::size_of::<le16>() / mem::size_of::<UsedElem>();
        let ptr = NonNull::new(ptr).unwrap().cast::<UsedElem>();
        NonNull::slice_from_raw_parts(ptr, len)
    }

    pub fn ring(&self) -> &[UsedElem] {
        let ptr = Self::ring_ptr(NonNull::from(self));
        unsafe { ptr.as_ref() }
    }

    pub fn ring_mut(&mut self) -> &mut [UsedElem] {
        let mut ptr = Self::ring_ptr(NonNull::from(self));
        unsafe { ptr.as_mut() }
    }

    pub fn avail_event_ptr(this: NonNull<Self>) -> Option<NonNull<le16>> {
        let ptr = unsafe { addr_of_mut!((*this.as_ptr()).ring_and_avail_event) };

        if ptr.len() * mem::size_of::<le16>() % mem::size_of::<UsedElem>() != mem::size_of::<le16>()
        {
            return None;
        }

        let start = ptr as *mut le16;
        let ptr = unsafe { start.add(ptr.len() - 1) };
        Some(NonNull::new(ptr).unwrap())
    }

    pub fn avail_event(&self) -> Option<&le16> {
        Self::avail_event_ptr(NonNull::from(self)).map(|ptr| unsafe { ptr.as_ref() })
    }

    pub fn avail_event_mut(&mut self) -> Option<&mut le16> {
        Self::avail_event_ptr(NonNull::from(self)).map(|mut ptr| unsafe { ptr.as_mut() })
    }
}

endian_bitflags! {
    /// Virtqueue used ring flags
    #[doc(alias = "VIRTQ_USED_F")]
    pub struct UsedF: le16 {
        /// The device uses this in used->flags to advise the driver: don’t kick me
        /// when you add a buffer.  It’s unreliable, so it’s simply an
        /// optimization.
        #[doc(alias = "VIRTQ_USED_F_NO_NOTIFY")]
        const NO_NOTIFY = 1;
    }
}

/// Used Ring Entry
#[doc(alias = "virtq_used_elem")]
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct UsedElem {
    /// Index of start of used descriptor chain.
    ///
    /// [`le32`] is used here for ids for padding reasons.
    pub id: le32,

    /// The number of bytes written into the device writable portion of
    /// the buffer described by the descriptor chain.
    pub len: le32,
}
