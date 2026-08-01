use alloc::collections::VecDeque;
use core::num::NonZeroU8;

use hermit_sync::{InterruptTicketMutex, Lazy};
use x86_64::instructions::port::Port;

use crate::kernel::interrupts;
use crate::synch::semaphore::Semaphore;

const PS2_DATA_PORT: u16 = 0x60;
const PS2_CMD_PORT: u16 = 0x64;
const PS2_CMD_READ_CNFG: u8 = 0x20;
const PS2_CMD_WRITE_CNFG: u8 = 0x60;
const PS2_CMD_DISABLE_KEYBOARD: u8 = 0xad;
const PS2_CMD_DISABLE_MOUSE: u8 = 0xa7;
const PS2_CMD_ENABLE_KEYBOARD: u8 = 0xae;
#[allow(dead_code)]
const PS2_CMD_ENABLE_MOUSE: u8 = 0xa8;
const PS2_CNFG_ENABLE_KEYBOARD_INTERRUPT: u8 = 0x01;
const PS2_BUFFER_FULL: u8 = 0x01;

const BUFFER_SIZE: usize = 256;
static KEYBOARD_SEMAPHORE: Semaphore = Semaphore::new(0);

struct Ps2;
impl Ps2 {
	pub fn read_status() -> u8 {
		unsafe { Port::<u8>::new(PS2_CMD_PORT).read() }
	}

	pub fn write_cmd(cmd: u8) {
		unsafe { Port::<u8>::new(PS2_CMD_PORT).write(cmd) }
	}

	pub fn read_data() -> u8 {
		unsafe { Port::<u8>::new(PS2_DATA_PORT).read() }
	}

	pub fn write_data(data: u8) {
		unsafe { Port::<u8>::new(PS2_DATA_PORT).write(data) }
	}
}

static KEYBOARD_BUFFER: Lazy<InterruptTicketMutex<VecDeque<NonZeroU8>>> =
	Lazy::new(|| InterruptTicketMutex::new(VecDeque::with_capacity(BUFFER_SIZE)));

fn keyboard_handler() {
	let scancode = Ps2::read_data();
	if let Some(valid_scancode) = NonZeroU8::new(scancode) {
		let mut sem = true;
		{
			let mut buffer = KEYBOARD_BUFFER.lock();

			// Pop the oldest scancode if the buffer is full.
			if buffer.len() >= BUFFER_SIZE {
				buffer.pop_front();
				sem = false;
			}
			buffer.push_back(valid_scancode);
		}
		if sem {
			KEYBOARD_SEMAPHORE.release();
		}
	}
}

pub(crate) fn get_keyboard_handler() -> (u8, fn()) {
	Ps2::write_cmd(PS2_CMD_DISABLE_KEYBOARD);
	Ps2::write_cmd(PS2_CMD_DISABLE_MOUSE);

	// Ensure an empty buffer to guard against stuck/garbage data
	while (Ps2::read_status() & PS2_BUFFER_FULL) != 0 {
		let _ = Ps2::read_data();
	}

	Ps2::write_cmd(PS2_CMD_READ_CNFG);
	let mut config = Ps2::read_data();

	config |= PS2_CNFG_ENABLE_KEYBOARD_INTERRUPT;

	Ps2::write_cmd(PS2_CMD_WRITE_CNFG);

	Ps2::write_data(config);
	Ps2::write_cmd(PS2_CMD_ENABLE_KEYBOARD);

	// Force the initialization of the keyboard buffer to ensure it is ready before any interrupts occur.
	Lazy::force(&KEYBOARD_BUFFER);

	interrupts::add_irq_name(1, "PS/2 Keyboard");

	(1, keyboard_handler)
}

/// Pops scancodes from the keyboard buffer into the provided slice. If `nonblocking` is false, the
/// function will sleep the current thread until a scancode has been received. Returns the number of scancodes
/// popped into the slice.
pub fn pop_scancodes(slice: &mut [u8], nonblocking: bool) -> usize {
	if slice.is_empty() {
		return 0;
	}
	if nonblocking {
		if !KEYBOARD_SEMAPHORE.try_acquire() {
			return 0;
		}
	} else {
		KEYBOARD_SEMAPHORE.acquire(None);
	}
	let mut amount: usize = 1;
	while amount < slice.len() && KEYBOARD_SEMAPHORE.try_acquire() {
		amount += 1;
	}
	let mut buffer = KEYBOARD_BUFFER.lock();
	for scancode in slice[..amount].iter_mut() {
		*scancode = buffer.pop_front().unwrap().get();
	}
	amount
}
