use crate::fd::ObjectInterface;
use crate::{entropy, io};

pub struct RandomFile;

impl ObjectInterface for RandomFile {
	async fn read(&self, buf: &mut [u8]) -> io::Result<usize> {
		entropy::read(buf, entropy::Flags::empty())
	}
}
