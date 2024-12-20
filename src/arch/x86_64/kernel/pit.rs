#![allow(dead_code)]

use x86_64::instructions::port::Port;

use crate::arch::x86_64::kernel::pic;

const PIT_CLOCK: u64 = 1_193_182;
pub const PIT_INTERRUPT_NUMBER: u8 = pic::PIC1_INTERRUPT_OFFSET;

const PIT_CHANNEL0_DATA: Port<u8> = Port::new(0x40);
const PIT_CHANNEL1_DATA: Port<u8> = Port::new(0x41);
const PIT_CHANNEL2_DATA: Port<u8> = Port::new(0x42);
const PIT_COMMAND: Port<u8> = Port::new(0x43);

const PIT_BINARY_OUTPUT: u8 = 0b0000_0000;
const PIT_BCD_OUTPUT: u8 = 0b0000_0001;

const PIT_COUNTDOWN_MODE: u8 = 0b0000_0000;
const PIT_ONESHOT_MODE: u8 = 0b0000_0010;
const PIT_RATE_GENERATOR_MODE: u8 = 0b0000_0100;
const PIT_SQUARE_WAVE_GENERATOR_MODE: u8 = 0b0000_0110;
const PIT_SW_TRIGGERED_STROBE_MODE: u8 = 0b0000_1000;
const PIT_HW_TRIGGERED_STROBE_MODE: u8 = 0b0000_1010;

const PIT_LOBYTE_ACCESS: u8 = 0b0001_0000;
const PIT_HIBYTE_ACCESS: u8 = 0b0010_0000;

const PIT_CHANNEL0: u8 = 0b0000_0000;
const PIT_CHANNEL1: u8 = 0b0100_0000;
const PIT_CHANNEL2: u8 = 0b1000_0000;

pub fn init(frequency_in_hz: u64) {
	pic::unmask(PIT_INTERRUPT_NUMBER);

	let mut pit_command = PIT_COMMAND;
	let mut pit_channel0_data = PIT_CHANNEL0_DATA;

	unsafe {
		// Reset the Programmable Interval Timer (PIT).
		pit_command.write(
			PIT_BINARY_OUTPUT
				| PIT_RATE_GENERATOR_MODE
				| PIT_LOBYTE_ACCESS
				| PIT_HIBYTE_ACCESS
				| PIT_CHANNEL0,
		);

		// Calculate the reload value to count down (round it to the closest integer).
		// Then transmit it as two individual bytes to the PIT.
		let count = (PIT_CLOCK + frequency_in_hz / 2) / frequency_in_hz;
		pit_channel0_data.write(count as u8);
		pit_channel0_data.write((count >> 8) as u8);
	}
}

pub fn deinit() {
	pic::mask(PIT_INTERRUPT_NUMBER);
}
