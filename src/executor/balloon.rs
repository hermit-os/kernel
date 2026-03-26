use core::future;
use core::task::Poll;

use crate::drivers::pci;
use crate::executor::spawn;

async fn balloon_run() {
	future::poll_fn(|cx| {
		if let Some(driver) = pci::get_balloon_driver() {
			let Some(mut driver_guard) = driver.try_lock() else {
				debug!(
					"Balloon driver was polled while the driver was locked elsewhere, doing nothing"
				);
				// This should only happen when polling while another core is deflating due to an OOM event,
				// or an interrupt is being handled, otherwise we only lock the driver here.

				// Interrupt handling should wake the registered waker and deflation as a result of OOM
				// handling should cause items to be submitted to the deflateq which should lead to a
				// future interrupt.
				return Poll::Pending;
			};

			driver_guard.poll_events(cx);

			Poll::Pending
		} else {
			Poll::Ready(())
		}
	})
	.await;
}

pub(crate) fn init() {
	info!("Try to initialize balloon interface!");

	if let Some(driver) = pci::get_balloon_driver() {
		driver.lock().enable_interrupts();
	}

	spawn(balloon_run());
}
