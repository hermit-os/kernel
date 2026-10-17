use std::path::{Path, PathBuf};

use anyhow::Result;
use xshell::cmd;

pub struct Object(PathBuf);

impl From<PathBuf> for Object {
	fn from(object: PathBuf) -> Self {
		Self(object)
	}
}

impl From<Object> for PathBuf {
	fn from(value: Object) -> Self {
		value.0
	}
}

impl AsRef<Path> for Object {
	fn as_ref(&self) -> &Path {
		&self.0
	}
}

impl Object {
	pub fn convert_to_elf32_i386(&self) -> Result<()> {
		let sh = crate::sh()?;
		let objcopy = crate::binutil("objcopy")?;
		let object = self.as_ref();
		cmd!(sh, "{objcopy} --output-target elf32-i386 {object}").run()?;
		Ok(())
	}
}
