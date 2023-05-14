use hermit_sync::InterruptTicketMutex;

use crate::drivers::net::NetworkInterface;

pub fn get_network_driver() -> Option<&'static InterruptTicketMutex<dyn NetworkInterface>> {
	None
}
