use lock_api::{GuardSend, RawMutex};

pub struct RawUnsoundMutex;

unsafe impl RawMutex for RawUnsoundMutex {
	const INIT: RawUnsoundMutex = RawUnsoundMutex;

	type GuardMarker = GuardSend;

	fn lock(&self) {}

	fn try_lock(&self) -> bool {
		true
	}

	unsafe fn unlock(&self) {}
}

pub type UnsoundMutex<T> = lock_api::Mutex<RawUnsoundMutex, T>;
