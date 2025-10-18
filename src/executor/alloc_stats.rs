use core::task::Poll;

use crate::executor::spawn;
use crate::mm::ALLOCATOR;

async fn print_alloc_stats() {
	core::future::poll_fn::<(), _>(|_cx| {
		let talc = ALLOCATOR.inner().lock();

		debug!("<alloc-stats>\n{}", talc.get_counters());

		Poll::Pending
	})
	.await;
}

pub(crate) fn init() {
	info!("Spawning allocation stats printing task");
	spawn(print_alloc_stats());
}
