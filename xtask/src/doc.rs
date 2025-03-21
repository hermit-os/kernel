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

			cmd!(sh, "cargo doc --target={triple}")
				.arg("--no-deps")
				.arg("--document-private-items")
				.arg("--package=hermit-kernel")
				.run()?;

			cmd!(sh, "cargo doc --target={triple}")
				.arg("--no-deps")
				.arg("--document-private-items")
				.arg("--manifest-path=hermit-builtins/Cargo.toml")
				.run()?;
		}

		cmd!(sh, "cargo doc")
			.arg("--no-deps")
			.arg("--document-private-items")
			.arg("--package=xtask")
			.run()?;

		Ok(())
	}
}
