use crate::arch::irq;
use crate::drivers::net::NetworkInterface;
use alloc::boxed::Box;

#[cfg(target_arch = "x86_64")]
use crate::arch::x86_64::kernel::irq::*;
#[cfg(target_arch = "x86_64")]
use crate::arch::x86_64::kernel::uhyve_get_ip;
#[cfg(target_arch = "x86_64")]
use crate::arch::x86_64::mm::paging::virt_to_phys;
#[cfg(target_arch = "x86_64")]
use crate::x86::io::*;

const UHYVE_IRQ_NET: u32 = 11;
const UHYVE_PORT_NETINFO: u16 = 0x600;

/// Data type to determine the mac address
#[derive(Debug, Default)]
#[repr(C)]
struct UhyveNetinfo {
	/// mac address
	pub mac: [u8; 6],
}

pub struct UhyveNetwork {
	/// mac address
	mac: [u8; 6],
}

impl UhyveNetwork {
	pub const fn new(mac: &[u8; 6]) -> Self {
		UhyveNetwork { mac: *mac }
	}
}

impl NetworkInterface for UhyveNetwork {
	fn get_mac_address(&self) -> [u8; 6] {
		self.mac
	}
}

pub fn init() -> Result<Box<dyn NetworkInterface>, ()> {
	// does uhyve configure the network interface?
	let ip = uhyve_get_ip();
	if ip[0] == 0xff && ip[1] == 0xff && ip[2] == 0xff && ip[3] == 0xff {
		return Err(());
	}

	debug!("Initialize uhyve network interface!");

	irq::disable();

	let nic = {
		let info: UhyveNetinfo = UhyveNetinfo::default();

		unsafe {
			outl(
				UHYVE_PORT_NETINFO,
				virt_to_phys(&info as *const _ as usize) as u32,
			);
		}

		Box::new(UhyveNetwork::new(&info.mac))
	};

	// Install interrupt handler
	irq_install_handler(
		UHYVE_IRQ_NET,
		crate::drivers::net::network_irqhandler as usize,
	);

	irq::enable();

	Ok(nic)
}
