mod text_buffer {
	#[derive(Clone, Copy, Debug)]
	#[repr(C)]
	pub struct Char {
		character: u8,
		attribute: u8,
	}

	impl Char {
		pub const fn new(byte: u8) -> Self {
			let character = if byte.is_ascii_graphic() || byte == b' ' {
				byte
			} else {
				b'?'
			};
			let light_grey = 0x07;
			Self {
				character,
				attribute: light_grey,
			}
		}

		pub const fn empty() -> Self {
			let black = 0x00;
			Self {
				character: b' ',
				attribute: black,
			}
		}
	}

	const WIDTH: usize = 80;
	const HEIGHT: usize = 25;
	const EMPTY_CHAR: Char = Char::empty();

	pub type TextBuffer = [[Char; WIDTH]; HEIGHT];
	pub const EMPTY_VGA_BUFFER: TextBuffer = [[EMPTY_CHAR; WIDTH]; HEIGHT];
}

mod front_buffer {
	use core::{fmt, ptr};

	use hermit_sync::CallOnce;
	use vcell::VolatileCell;
	use x86_64::instructions::port::Port;

	use super::text_buffer::TextBuffer;
	use crate::arch::x86_64::mm::paging::{
		BasePageSize, PageTableEntryFlags, PageTableEntryFlagsExt,
	};
	use crate::arch::x86_64::mm::{paging, PhysAddr, VirtAddr};

	pub struct FrontBuffer {
		buffer: &'static mut VolatileCell<TextBuffer>,
	}

	impl fmt::Debug for FrontBuffer {
		fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
			f.debug_struct("FrontBuffer").finish_non_exhaustive()
		}
	}

	impl FrontBuffer {
		pub fn take() -> Option<Self> {
			static TAKEN: CallOnce = CallOnce::new();
			TAKEN.call_once().ok()?;

			const ADDR: usize = 0xB8000;

			// Identity map the VGA buffer. We only need the first page.
			let mut flags = PageTableEntryFlags::empty();
			flags.device().writable().execute_disable();
			paging::map::<BasePageSize>(
				VirtAddr(ADDR.try_into().unwrap()),
				PhysAddr(ADDR.try_into().unwrap()),
				1,
				flags,
			);

			// Disable the cursor.
			unsafe {
				const CRT_CONTROLLER_ADDRESS_PORT: u16 = 0x3D4;
				const CRT_CONTROLLER_DATA_PORT: u16 = 0x3D5;
				const CURSOR_START_REGISTER: u8 = 0x0A;
				const CURSOR_DISABLE: u8 = 0x20;

				Port::new(CRT_CONTROLLER_ADDRESS_PORT).write(CURSOR_START_REGISTER);
				Port::new(CRT_CONTROLLER_DATA_PORT).write(CURSOR_DISABLE);
			}

			let buffer =
				unsafe { &mut *ptr::from_exposed_addr_mut::<VolatileCell<TextBuffer>>(ADDR) };

			Some(Self { buffer })
		}

		pub fn set(&mut self, buffer: TextBuffer) {
			self.buffer.set(buffer);
		}

		pub fn get(&mut self) -> TextBuffer {
			self.buffer.get()
		}
	}
}

mod back_buffer {
	use super::text_buffer::{Char, TextBuffer};

	#[derive(Debug)]
	pub struct BackBuffer<'a> {
		buffer: &'a mut TextBuffer,
		column: usize,
	}

	impl<'a> From<&'a mut TextBuffer> for BackBuffer<'a> {
		fn from(buffer: &'a mut TextBuffer) -> Self {
			Self { buffer, column: 0 }
		}
	}

	impl<'a> BackBuffer<'a> {
		pub fn get(&self) -> TextBuffer {
			*self.buffer
		}

		fn newline(&mut self) {
			let len = self.buffer.len();
			self.buffer.copy_within(1..len, 0);
			self.buffer.last_mut().unwrap().fill(Char::empty());
			self.column = 0;
		}

		fn write_byte(&mut self, byte: u8) {
			if byte == b'\n' {
				self.newline();
			} else {
				let line = self.buffer.last_mut().unwrap();

				line[self.column] = Char::new(byte);
				self.column += 1;

				if self.column == line.len() {
					self.newline();
				}
			}
		}

		pub fn write(&mut self, buf: &[u8]) {
			for &byte in buf {
				self.write_byte(byte);
			}
		}
	}
}

use hermit_sync::{ExclusiveCell, InterruptOnceCell, InterruptSpinMutex};

use self::back_buffer::BackBuffer;
use self::front_buffer::FrontBuffer;
use self::text_buffer::EMPTY_VGA_BUFFER;
use crate::arch::x86_64::kernel::vga::text_buffer::TextBuffer;

#[derive(Debug)]
struct VgaScreen {
	front_buffer: FrontBuffer,
	back_buffer: BackBuffer<'static>,
}

impl VgaScreen {
	fn take() -> Option<Self> {
		let mut front_buffer = FrontBuffer::take()?;

		static BACK_BUFFER: ExclusiveCell<TextBuffer> = ExclusiveCell::new(EMPTY_VGA_BUFFER);
		let back_buffer = BACK_BUFFER.take()?;
		*back_buffer = front_buffer.get();

		Some(Self {
			front_buffer,
			back_buffer: BackBuffer::from(back_buffer),
		})
	}

	fn print(&mut self, buf: &[u8]) {
		self.back_buffer.write(buf);
		self.front_buffer.set(self.back_buffer.get());
	}
}

static VGA_SCREEN: InterruptOnceCell<InterruptSpinMutex<VgaScreen>> = InterruptOnceCell::new();

pub fn init() {
	VGA_SCREEN
		.set(InterruptSpinMutex::new(VgaScreen::take().unwrap()))
		.unwrap();
}

pub fn print(buf: &[u8]) {
	if let Some(vga_screen) = VGA_SCREEN.get() {
		vga_screen.lock().print(buf);
	}
}
