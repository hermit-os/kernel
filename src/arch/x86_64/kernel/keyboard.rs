use core::sync::atomic::{AtomicU8, AtomicUsize, Ordering};

use x86_64::instructions::port::Port;

use crate::kernel::interrupts;

const BUFFER_SIZE: usize = 256;
#[allow(clippy::declare_interior_mutable_const)]
const ATOMIC_ZERO: AtomicU8 = AtomicU8::new(0);
const PS2_DATA_PORT: u16 = 0x60;
const PS2_CMD_PORT: u16 = 0x64;
const PS2_CMD_READ_CNFG: u8 = 0x20;
const PS2_CMD_WRITE_CNFG: u8 = 0x60;
const PS2_CMD_DISABLE_KEYBOARD: u8 = 0xad;
const PS2_CMD_DISABLE_MOUSE: u8 = 0xa7;
const PS2_CMD_ENABLE_KEYBOARD: u8 = 0xae;
const PS2_CNFG_ENABLE_KEYBOARD_INTERRUPT: u8 = 0x01;
const PS2_BUFFER_FULL: u8 = 0x01;

static KEYBOARD_BUFFER: [AtomicU8; BUFFER_SIZE] = [ATOMIC_ZERO; BUFFER_SIZE];
static WRITE_INDEX: AtomicUsize = AtomicUsize::new(0);
static READ_INDEX: AtomicUsize = AtomicUsize::new(0);

pub(crate) fn get_keyboard_handler() -> (u8, fn()) {
	unsafe {
		let mut cmd_port = Port::<u8>::new(PS2_CMD_PORT);
		let mut data_port = Port::<u8>::new(PS2_DATA_PORT);
		cmd_port.write(PS2_CMD_DISABLE_KEYBOARD);
		cmd_port.write(PS2_CMD_DISABLE_MOUSE);

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

		let write_idx = WRITE_INDEX.load(Ordering::Relaxed);
		let next_write_idx = write_idx.wrapping_add(1) % BUFFER_SIZE;

		let read_idx = READ_INDEX.load(Ordering::Acquire);
		if next_write_idx != read_idx {
			KEYBOARD_BUFFER[write_idx].store(scancode, Ordering::Release);
			WRITE_INDEX.store(next_write_idx, Ordering::Release);
		}
	}

	interrupts::add_irq_name(1, "PS/2 Keyboard");

	(1, keyboard_handler)
}

/// Pops a scancode from the keyboard buffer, returning None if the buffer is empty.
pub fn pop_scancode() -> Option<u8> {
	let read_idx = READ_INDEX.load(Ordering::Relaxed);
	let write_idx = WRITE_INDEX.load(Ordering::Acquire);

	if read_idx == write_idx {
		None
	} else {
		let scancode = KEYBOARD_BUFFER[read_idx].load(Ordering::Acquire);
		READ_INDEX.store(read_idx.wrapping_add(1) % BUFFER_SIZE, Ordering::Release);
		Some(scancode)
	}
}
