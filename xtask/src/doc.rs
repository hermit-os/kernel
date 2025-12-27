use std::ffi::OsStr;
use std::io;

use anyhow::Result;
use clap::Args;

use crate::arch::Arch;

/// Run rustdoc for all targets.
#[derive(Args)]
pub struct Doc;

impl Doc {
	pub fn run(self) -> Result<()> {
		for arch in Arch::all() {
			arch.install()?;
			let triple = arch.triple();
			let target = format!("--target={triple}");
			let target = target.as_str();

			doc([target, "--package=hermit-kernel"])?;
			doc([target, "--manifest-path=hermit-builtins/Cargo.toml"])?;
		}

		doc(["--package=xtask"])?;
		Ok(())
	}
}

fn doc<I, S>(args: I) -> io::Result<()>
where
	I: IntoIterator<Item = S>,
	S: AsRef<OsStr>,
{
	let mut cmd = crate::cargo();
	cmd.arg("doc");
	cmd.arg("--no-deps");
	cmd.arg("--document-private-items");
	cmd.args(args);

	eprintln!("$ {cmd:?}");
	cmd.spawn()?.wait()?;

	Ok(())
}
