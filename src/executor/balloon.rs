use core::future;
use core::task::Poll;

use crate::drivers::pci;
use crate::executor::spawn;

async fn balloon_run() {
	future::poll_fn(|_cx| {
		if let Some(driver) = pci::get_balloon_driver() {
			let Some(mut driver_guard) = driver.try_lock() else {
				debug!(
					"Balloon driver was polled while the driver was locked elsewhere, doing nothing"
				);
				return Poll::Pending;
			};

			driver_guard.poll_events();

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
