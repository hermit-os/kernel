mod console;
mod uhyve;

use alloc::sync::Arc;

use ahash::RandomState;
use hashbrown::HashMap;

pub use self::console::{ConsoleStderr, ConsoleStdin, ConsoleStdout};
pub use self::uhyve::{UhyveStderr, UhyveStdin, UhyveStdout};
use crate::fd::{Fd, RawFd, STDERR_FILENO, STDIN_FILENO, STDOUT_FILENO};

pub(crate) fn setup(fds: &mut HashMap<RawFd, Arc<async_lock::RwLock<Fd>>, RandomState>) {
	if crate::env::is_uhyve() {
		let stdin = Arc::new(async_lock::RwLock::new(UhyveStdin::new().into()));
		let stdout = Arc::new(async_lock::RwLock::new(UhyveStdout::new().into()));
		let stderr = Arc::new(async_lock::RwLock::new(UhyveStderr::new().into()));
		fds.insert(STDIN_FILENO, stdin);
		fds.insert(STDOUT_FILENO, stdout);
		fds.insert(STDERR_FILENO, stderr);
		return;
	}

	let stdin = Arc::new(async_lock::RwLock::new(ConsoleStdin::new().into()));
	let stdout = Arc::new(async_lock::RwLock::new(ConsoleStdout::new().into()));
	let stderr = Arc::new(async_lock::RwLock::new(ConsoleStderr::new().into()));
	fds.insert(STDIN_FILENO, stdin);
	fds.insert(STDOUT_FILENO, stdout);
	fds.insert(STDERR_FILENO, stderr);
}
