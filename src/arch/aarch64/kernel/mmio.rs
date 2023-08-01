use hermit_sync::InterruptTicketMutex;

use crate::drivers::net::virtio_net::VirtioNetDriver;

pub(crate) fn get_network_driver() -> Option<&'static InterruptTicketMutex<VirtioNetDriver>> {
	None
}
