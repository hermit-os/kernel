// Copyright (c) 2017 Stefan Lankes, RWTH Aachen University
//                    Colin Finck, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

#![allow(dead_code)]

use arch::x86_64::kernel::pic;
use x86::io::*;

const PIT_CLOCK: u64 = 1193182;
pub const PIT_INTERRUPT_NUMBER: u8 = pic::PIC1_INTERRUPT_OFFSET + 0;

const PIT_CHANNEL0_DATA_PORT: u16 = 0x40;
const PIT_CHANNEL1_DATA_PORT: u16 = 0x41;
const PIT_CHANNEL2_DATA_PORT: u16 = 0x42;
const PIT_COMMAND_PORT: u16 = 0x43;

const PIT_BINARY_OUTPUT: u8 = 0b00000000;
const PIT_BCD_OUTPUT: u8 = 0b00000001;

const PIT_COUNTDOWN_MODE: u8 = 0b00000000;
const PIT_ONESHOT_MODE: u8 = 0b00000010;
const PIT_RATE_GENERATOR_MODE: u8 = 0b00000100;
const PIT_SQUARE_WAVE_GENERATOR_MODE: u8 = 0b00000110;
const PIT_SW_TRIGGERED_STROBE_MODE: u8 = 0b00001000;
const PIT_HW_TRIGGERED_STROBE_MODE: u8 = 0b00001010;

const PIT_LOBYTE_ACCESS: u8 = 0b00010000;
const PIT_HIBYTE_ACCESS: u8 = 0b00100000;

const PIT_CHANNEL0: u8 = 0b00000000;
const PIT_CHANNEL1: u8 = 0b01000000;
const PIT_CHANNEL2: u8 = 0b10000000;

pub fn init(frequency_in_hz: u64) {
    pic::unmask(PIT_INTERRUPT_NUMBER);

    unsafe {
        // Reset the Programmable Interval Timer (PIT).
        outb(
            PIT_COMMAND_PORT,
            PIT_BINARY_OUTPUT
                | PIT_RATE_GENERATOR_MODE
                | PIT_LOBYTE_ACCESS
                | PIT_HIBYTE_ACCESS
                | PIT_CHANNEL0,
        );

        // Calculate the reload value to count down (round it to the closest integer).
        // Then transmit it as two individual bytes to the PIT.
        let count = (PIT_CLOCK + frequency_in_hz / 2) / frequency_in_hz;
        outb(PIT_CHANNEL0_DATA_PORT, count as u8);
        outb(PIT_CHANNEL0_DATA_PORT, (count >> 8) as u8);
    }
}

pub fn deinit() {
    pic::mask(PIT_INTERRUPT_NUMBER);
}
