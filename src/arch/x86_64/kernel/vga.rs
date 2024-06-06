use core::alloc::Layout;

use core::fmt::Write;
use hermit_sync::{InterruptOnceCell, InterruptSpinMutex};
use vga_text_mode::{ VgaScreen, FrontBuffer, text_buffer::TextBuffer};
use x86_64::instructions::port::Port;

use crate::arch::x86_64::mm::paging::{BasePageSize, PageTableEntryFlags, PageTableEntryFlagsExt};
use crate::arch::x86_64::mm::{paging, PhysAddr, VirtAddr};

static VGA_SCREEN: InterruptOnceCell<InterruptSpinMutex<VgaScreen<'static>>> = InterruptOnceCell::new();

pub fn init() {
	let layout = Layout::new::<TextBuffer>();
	let virt_addr = FrontBuffer::PHYS_ADDR;

	// Identity map the VGA buffer. We only need the first page.
	let mut flags = PageTableEntryFlags::empty();
	flags.device().writable().execute_disable();
	paging::map::<BasePageSize>(
		VirtAddr(virt_addr as _),
		PhysAddr(FrontBuffer::PHYS_ADDR as u64),
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

	let front_buffer = unsafe { &mut *(virt_addr as *mut TextBuffer) };
	let front_buffer = FrontBuffer::new(front_buffer);

	VGA_SCREEN
		.set(InterruptSpinMutex::new(VgaScreen::new(front_buffer)))
		.unwrap();
}

pub fn print(s: &str) {
	if let Some(vga_screen) = VGA_SCREEN.get() {
		vga_screen.lock().write_str(s).unwrap();
	}
}
