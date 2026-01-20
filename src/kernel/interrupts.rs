use ahash::RandomState;
use hashbrown::HashMap;
use hermit_sync::InterruptTicketMutex;

use super::SPI_START;

pub(super) static IRQ_NAMES: InterruptTicketMutex<HashMap<u8, &'static str, RandomState>> =
	InterruptTicketMutex::new(HashMap::with_hasher(RandomState::with_seeds(0, 0, 0, 0)));

#[allow(dead_code)]
pub(crate) fn add_irq_name(irq_number: u8, name: &'static str) {
	debug!("Register name \"{name}\" for interrupt {irq_number}");
	IRQ_NAMES.lock().insert(SPI_START + irq_number, name);
}

#[allow(dead_code)]
pub(super) fn get_irq_name(irq_number: u8) -> Option<&'static str> {
	IRQ_NAMES.lock().get(&irq_number).copied()
}
