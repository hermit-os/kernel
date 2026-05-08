use alloc::boxed::Box;
use alloc::sync::Arc;

use crate::errno::Errno;
use crate::fd::random_file::RandomFile;
use crate::fd::{AccessPermission, Fd, OpenOption};
use crate::fs::{NodeKind, VfsNode};
use crate::io;

pub(crate) fn init() {
	crate::fs::FILESYSTEM
		.get()
		.unwrap()
		.mount("/dev", Box::new(DevDirectory))
		.unwrap();
}

#[derive(Debug)]
pub(crate) struct DevDirectory;

impl VfsNode for DevDirectory {
	fn get_kind(&self) -> NodeKind {
		NodeKind::Directory
	}

	fn traverse_open(
		&self,
		path: &str,
		_option: OpenOption,
		_mode: AccessPermission,
	) -> io::Result<Arc<async_lock::RwLock<Fd>>> {
		match path {
			"urandom" | "random" => Ok(Arc::new(async_lock::RwLock::new(Fd::RandomFile(
				RandomFile,
			)))),
			_ => Err(Errno::Noent),
		}
	}
}
