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

		for arch in Arch::all() {
			arch.install()?;
			let triple = arch.triple();
			cmd!(
				sh,
				"cargo doc --package hermit-kernel --no-deps --document-private-items --target={triple}"
			)
			.run()?;
		}

		Ok(())
	}
}
