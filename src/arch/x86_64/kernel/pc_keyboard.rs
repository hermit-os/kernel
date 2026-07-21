use alloc::collections::VecDeque;

use hermit_sync::{InterruptTicketMutex, Lazy};
use x86_64::instructions::port::Port;

use crate::kernel::interrupts;

const PS2_DATA_PORT: u16 = 0x60;
const PS2_CMD_PORT: u16 = 0x64;
const PS2_CMD_READ_CNFG: u8 = 0x20;
const PS2_CMD_WRITE_CNFG: u8 = 0x60;
const PS2_CMD_DISABLE_KEYBOARD: u8 = 0xad;
const PS2_CMD_DISABLE_MOUSE: u8 = 0xa7;
const PS2_CMD_ENABLE_KEYBOARD: u8 = 0xae;
const PS2_CNFG_ENABLE_KEYBOARD_INTERRUPT: u8 = 0x01;
const PS2_BUFFER_FULL: u8 = 0x01;

const BUFFER_SIZE: usize = 256;

static KEYBOARD_BUFFER: Lazy<InterruptTicketMutex<VecDeque<u8>>> =
	Lazy::new(|| InterruptTicketMutex::new(VecDeque::with_capacity(BUFFER_SIZE)));

pub(crate) fn get_keyboard_handler() -> (u8, fn()) {
	let mut cmd_port = Port::<u8>::new(PS2_CMD_PORT);
	let mut data_port = Port::<u8>::new(PS2_DATA_PORT);

	unsafe {
		cmd_port.write(PS2_CMD_DISABLE_KEYBOARD);
		cmd_port.write(PS2_CMD_DISABLE_MOUSE);

		// Clear garbage data from the PS/2 buffer
		while (cmd_port.read() & PS2_BUFFER_FULL) != 0 {
			let _ = data_port.read();
		}

		cmd_port.write(PS2_CMD_READ_CNFG);
		let mut config = data_port.read();

		config |= PS2_CNFG_ENABLE_KEYBOARD_INTERRUPT;

		cmd_port.write(PS2_CMD_WRITE_CNFG);
		data_port.write(config);
		cmd_port.write(PS2_CMD_ENABLE_KEYBOARD);
	}

	fn keyboard_handler() {
		let mut data_port = Port::<u8>::new(PS2_DATA_PORT);
		let scancode = unsafe { data_port.read() };
		let mut buffer = KEYBOARD_BUFFER.lock();

		if buffer.len() >= BUFFER_SIZE {
			buffer.pop_front();
		}
		buffer.push_back(scancode);
	}

	// Force the initialization of the keyboard buffer to ensure it is ready before any interrupts occur.
	Lazy::force(&KEYBOARD_BUFFER);

	interrupts::add_irq_name(1, "PS/2 Keyboard");

	(1, keyboard_handler)
}

/// Pops a scancode from the keyboard buffer, returning None if the buffer is empty.
pub fn pop_scancode() -> Option<u8> {
	KEYBOARD_BUFFER.lock().pop_front()
}
