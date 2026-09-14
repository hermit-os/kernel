use crate::fd::ObjectInterface;
use crate::io;

pub struct NullFile;

impl ObjectInterface for NullFile {
	async fn read(&self, _buf: &mut [u8]) -> io::Result<usize> {
		Ok(0)
	}

	async fn write(&self, buf: &[u8]) -> io::Result<usize> {
		Ok(buf.len())
	}
}
