use core::future;
use core::task::Poll;

use crate::executor::spawn;
use crate::mm::ALLOCATOR;

async fn print_alloc_stats() {
	future::poll_fn(|cx| {
		let talc = ALLOCATOR.lock();

		debug!("<alloc-stats>\n{}", talc.get_counters());

		cx.waker().wake_by_ref();
		Poll::<()>::Pending
	})
	.await;
}

pub(crate) fn init() {
	info!("Spawning allocation stats printing task");
	spawn(print_alloc_stats());
}
