//! See <https://github.com/matklad/cargo-xtask/>.

mod arch;
mod archive;
mod build;
mod clippy;
mod flags;

use std::path::Path;

use anyhow::Result;
use xshell::Shell;

fn main() -> Result<()> {
	flags::Xtask::from_env()?.run()
}

impl flags::Xtask {
	fn run(self) -> Result<()> {
		match self.subcommand {
			flags::XtaskCmd::Build(build) => build.run(),
			flags::XtaskCmd::Clippy(clippy) => clippy.run(),
		}
	}
}

pub fn sh() -> Result<Shell> {
	let sh = Shell::new()?;
	let project_root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
	sh.change_dir(project_root);
	Ok(sh)
}
