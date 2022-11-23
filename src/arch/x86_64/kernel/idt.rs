use x86_64::structures::idt::InterruptDescriptorTable;

pub static mut IDT: InterruptDescriptorTable = InterruptDescriptorTable::new();

pub fn install() {
	unsafe {
		IDT.load_unsafe();
	}
}
