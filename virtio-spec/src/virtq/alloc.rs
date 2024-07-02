use ::alloc::alloc::handle_alloc_error;
use allocator_api2::alloc::{AllocError, Allocator, Global};
use allocator_api2::boxed::Box;

use super::*;

impl Avail {
    pub fn new(queue_size: u16, has_event_idx: bool) -> Box<Self> {
        Self::new_in(queue_size, has_event_idx, Global)
    }

    pub fn try_new(queue_size: u16, has_event_idx: bool) -> Result<Box<Self>, AllocError> {
        Self::try_new_in(queue_size, has_event_idx, Global)
    }

    pub fn new_in<A: Allocator>(queue_size: u16, has_event_idx: bool, alloc: A) -> Box<Self, A> {
        Self::try_new_in(queue_size, has_event_idx, alloc)
            .unwrap_or_else(|_| handle_alloc_error(Self::layout(queue_size, has_event_idx)))
    }

    pub fn try_new_in<A: Allocator>(
        queue_size: u16,
        has_event_idx: bool,
        alloc: A,
    ) -> Result<Box<Self, A>, AllocError> {
        let layout = Self::layout(queue_size, has_event_idx);

        let mem = alloc.allocate_zeroed(layout)?;
        let mem = NonNull::slice_from_raw_parts(mem.cast(), layout.size());
        let raw = Self::from_ptr(mem).unwrap();
        let boxed = unsafe { Box::from_raw_in(raw.as_ptr(), alloc) };

        debug_assert_eq!(Layout::for_value(&*boxed), layout);
        debug_assert_eq!(boxed.ring(has_event_idx).len(), queue_size.into());
        debug_assert_eq!(boxed.used_event(has_event_idx).is_some(), has_event_idx);

        Ok(boxed)
    }
}

impl Used {
    pub fn new(queue_size: u16, has_event_idx: bool) -> Box<Self> {
        Self::new_in(queue_size, has_event_idx, Global)
    }

    pub fn try_new(queue_size: u16, has_event_idx: bool) -> Result<Box<Self>, AllocError> {
        Self::try_new_in(queue_size, has_event_idx, Global)
    }

    pub fn new_in<A: Allocator>(queue_size: u16, has_event_idx: bool, alloc: A) -> Box<Self, A> {
        Self::try_new_in(queue_size, has_event_idx, alloc)
            .unwrap_or_else(|_| handle_alloc_error(Self::layout(queue_size, has_event_idx)))
    }

    pub fn try_new_in<A: Allocator>(
        queue_size: u16,
        has_event_idx: bool,
        alloc: A,
    ) -> Result<Box<Self, A>, AllocError> {
        let layout = Self::layout(queue_size, has_event_idx);

        let mem = alloc.allocate_zeroed(layout)?;
        let mem = NonNull::slice_from_raw_parts(mem.cast(), layout.size());
        let raw = Self::from_ptr(mem, has_event_idx).unwrap();
        let boxed = unsafe { Box::from_raw_in(raw.as_ptr(), alloc) };

        debug_assert_eq!(Layout::for_value(&*boxed), layout);
        debug_assert_eq!(boxed.ring().len(), queue_size.into());
        debug_assert_eq!(boxed.avail_event().is_some(), has_event_idx);

        Ok(boxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn avail_layout() {
        for queue_size in [255, 256, 257] {
            for has_event_idx in [false, true] {
                Avail::new(queue_size, has_event_idx);
            }
        }
    }

    #[test]
    fn used_layout() {
        for queue_size in [255, 256, 257] {
            for has_event_idx in [false, true] {
                Used::new(queue_size, has_event_idx);
            }
        }
    }
}
