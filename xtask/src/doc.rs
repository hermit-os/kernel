use anyhow::Result;
use clap::Args;
use xshell::cmd;

use crate::arch::Arch;

/// Run rustdoc for all targets.
#[derive(Args)]
pub struct Doc;

impl Doc {
	pub fn run(self) -> Result<()> {
		let sh = crate::sh()?;

		let mut doc = cmd!(
			sh,
			"cargo doc --package hermit-kernel --no-deps --document-private-items"
		);

		for arch in Arch::all() {
			arch.install()?;
			let triple = arch.triple();
			doc = doc.arg(format!("--target={triple}"));
		}

		doc.run()?;

		Ok(())
	}
}
