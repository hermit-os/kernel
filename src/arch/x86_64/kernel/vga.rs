use core::ptr;

use hermit_sync::SpinMutex;
use memory_addresses::{PhysAddr, VirtAddr};
use x86_64::instructions::port::Port;

use crate::arch::x86_64::mm::paging;
use crate::arch::x86_64::mm::paging::{BasePageSize, PageTableEntryFlags, PageTableEntryFlagsExt};

const CRT_CONTROLLER_ADDRESS: Port<u8> = Port::new(0x3d4);
const CRT_CONTROLLER_DATA: Port<u8> = Port::new(0x3d5);
const CURSOR_START_REGISTER: u8 = 0x0a;
const CURSOR_DISABLE: u8 = 0x20;

const ATTRIBUTE_BLACK: u8 = 0x00;
const ATTRIBUTE_LIGHTGREY: u8 = 0x07;
const COLS: usize = 80;
const ROWS: usize = 25;
const VGA_BUFFER_ADDRESS: PhysAddr = PhysAddr::new(0xb8000);

static VGA_SCREEN: SpinMutex<VgaScreen> = SpinMutex::new(VgaScreen::new());

#[derive(Clone, Copy)]
#[repr(C, packed)]
struct VgaCharacter {
	character: u8,
	attribute: u8,
}

impl VgaCharacter {
	const fn new(character: u8, attribute: u8) -> Self {
		Self {
			character,
			attribute,
		}
	}
}

struct VgaScreen {
	buffer: *mut [[VgaCharacter; COLS]; ROWS],
	current_col: usize,
	current_row: usize,
	is_initialized: bool,
}

// FIXME: make `buffer` implement `Send` instead
unsafe impl Send for VgaScreen {}

impl VgaScreen {
	const fn new() -> Self {
		Self {
			buffer: ptr::with_exposed_provenance_mut(VGA_BUFFER_ADDRESS.as_usize()),
			current_col: 0,
			current_row: 0,
			is_initialized: false,
		}
	}

	fn init(&mut self) {
		// Identity map the VGA buffer. We only need the first page.
		let mut flags = PageTableEntryFlags::empty();
		flags.device().writable().execute_disable();
		paging::map::<BasePageSize>(
			VirtAddr::new(VGA_BUFFER_ADDRESS.as_u64()),
			VGA_BUFFER_ADDRESS,
			1,
			flags,
		);

		// Disable the cursor.
		let mut crt_controller_address = CRT_CONTROLLER_ADDRESS;
		let mut crt_controller_data = CRT_CONTROLLER_DATA;
		unsafe {
			crt_controller_address.write(CURSOR_START_REGISTER);
			crt_controller_data.write(CURSOR_DISABLE);
		}

		// Clear the screen.
		for r in 0..ROWS {
			self.clear_row(r);
		}

		// Initialization done!
		self.is_initialized = true;
	}

	#[inline]
	fn clear_row(&mut self, row: usize) {
		// Overwrite this row by a bogus character in black.
		for c in 0..COLS {
			unsafe {
				(*self.buffer)[row][c] = VgaCharacter::new(0, ATTRIBUTE_BLACK);
			}
		}
	}

	fn write_byte(&mut self, byte: u8) {
		if !self.is_initialized {
			return;
		}

		// Move to the next row if we have a newline character or hit the end of a column.
		if byte == b'\n' || self.current_col == COLS {
			self.current_col = 0;
			self.current_row += 1;
		}

		// Check if we have hit the end of the screen rows.
		if self.current_row == ROWS {
			// Shift all rows up by one line, removing the oldest visible screen row.
			for r in 1..ROWS {
				for c in 0..COLS {
					unsafe {
						(*self.buffer)[r - 1][c] = (*self.buffer)[r][c];
					}
				}
			}

			// Clear the last screen row and write to it next time.
			self.clear_row(ROWS - 1);
			self.current_row = ROWS - 1;
		}

		if byte != b'\n' {
			// Put our character into the VGA screen buffer and advance the column counter.
			unsafe {
				(*self.buffer)[self.current_row][self.current_col] =
					VgaCharacter::new(byte, ATTRIBUTE_LIGHTGREY);
			}
			self.current_col += 1;
		}
	}
}

pub fn init() {
	VGA_SCREEN.lock().init();
}

pub fn write_byte(byte: u8) {
	VGA_SCREEN.lock().write_byte(byte);
}
