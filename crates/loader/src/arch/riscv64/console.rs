use sbi_rt::Physical;

#[derive(Default)]
pub struct Console(());

impl Console {
	pub fn write_bytes(&mut self, bytes: &[u8]) {
		sbi_rt::console_write(Physical::new(
			bytes.len(),
			bytes.as_ptr().expose_provenance(),
			0,
		));
	}
}
