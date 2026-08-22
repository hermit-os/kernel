use alloc::collections::VecDeque;
use core::num::NonZeroU8;

use hermit_sync::{InterruptTicketMutex, Lazy};
use x86_64::instructions::port::Port;

use crate::arch::kernel::interrupts;
use crate::synch::semaphore::Semaphore;

const PS2_DATA_PORT: u16 = 0x60;
const PS2_CMD_PORT: u16 = 0x64;

#[repr(u8)]
enum Ps2Command {
	ReadConfig = 0x20,
	WriteConfig = 0x60,
	DisableKeyboard = 0xad,
	DisableMouse = 0xa7,
	EnableKeyboard = 0xae,
	#[allow(dead_code)]
	EnableMouse = 0xa8,
	TestFirstPort = 0xab,
}

const PS2_CNFG_ENABLE_KEYBOARD_INTERRUPT: u8 = 0x01;
const PS2_BUFFER_FULL: u8 = 0x01;

const MAX_INP_BUFFER_SIZE: usize = 256;
static KEYBOARD_SEMAPHORE: Semaphore = Semaphore::new(0);

struct Ps2;
impl Ps2 {
	pub fn read_status() -> u8 {
		// SAFETY: Correct port access without safety related side-effects.
		unsafe { Port::<u8>::new(PS2_CMD_PORT).read() }
	}

	pub fn write_cmd(cmd: Ps2Command) {
		// SAFETY: Correct port access without memory safety related side-effects.
		unsafe { Port::<u8>::new(PS2_CMD_PORT).write(cmd as u8) }
	}

	pub fn read_data() -> u8 {
		// SAFETY: Correct port access without safety related side-effects.
		unsafe { Port::<u8>::new(PS2_DATA_PORT).read() }
	}

	pub fn write_data(data: u8) {
		// SAFETY: Correct port access without memory safety related side-effects.
		unsafe { Port::<u8>::new(PS2_DATA_PORT).write(data) }
	}
}

static KEYBOARD_BUFFER: Lazy<InterruptTicketMutex<VecDeque<NonZeroU8>>> =
	Lazy::new(|| InterruptTicketMutex::new(VecDeque::with_capacity(32)));

fn keyboard_handler() {
	let scancode = Ps2::read_data();
	if let Some(valid_scancode) = NonZeroU8::new(scancode) {
		{
			let mut buffer = KEYBOARD_BUFFER.lock();

			// Pop the oldest scancode if the buffer is full.
			if buffer.len() >= MAX_INP_BUFFER_SIZE {
				buffer.pop_front();
				buffer.push_back(valid_scancode);
				return;
			}
			buffer.push_back(valid_scancode);
		}
		KEYBOARD_SEMAPHORE.release();
	}
}

pub(crate) fn get_keyboard_handler() -> (u8, fn()) {
	Ps2::write_cmd(Ps2Command::DisableKeyboard);
	Ps2::write_cmd(Ps2Command::DisableMouse);

	// Ensure an empty buffer to guard against stuck/garbage data
	while (Ps2::read_status() & PS2_BUFFER_FULL) != 0 {
		let _ = Ps2::read_data();
	}

	Ps2::write_cmd(Ps2Command::ReadConfig);
	let mut config = Ps2::read_data();

	config |= PS2_CNFG_ENABLE_KEYBOARD_INTERRUPT;

	Ps2::write_cmd(Ps2Command::WriteConfig);
	Ps2::write_data(config);

	Ps2::write_cmd(Ps2Command::TestFirstPort);

	if Ps2::read_data() != 0 {
		error!("PS/2 keyboard test failed");
	}

	Ps2::write_cmd(Ps2Command::EnableKeyboard);

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
