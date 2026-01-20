use async_executor::StaticExecutor;
use hermit_sync::{RawRwSpinLock, RawSpinMutex};

use super::CoreLocal;
use crate::scheduler::{CoreId, PerCoreScheduler};

#[inline]
pub(crate) fn core_id() -> CoreId {
	if cfg!(target_os = "none") {
		CoreLocal::get().core_id
	} else {
		0
	}
}

#[inline]
pub(crate) fn core_scheduler() -> &'static mut PerCoreScheduler {
	unsafe { CoreLocal::get().scheduler.get().as_mut().unwrap() }
}

#[inline]
pub fn set_core_scheduler(scheduler: *mut PerCoreScheduler) {
	CoreLocal::get().scheduler.set(scheduler);
}

#[inline]
pub(crate) fn ex() -> &'static StaticExecutor<RawSpinMutex, RawRwSpinLock> {
	&CoreLocal::get().ex
}
