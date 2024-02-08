use anyhow::Result;
use clap::Args;
use xshell::cmd;

use crate::target::Target;

/// Run Clippy for all targets.
#[derive(Args)]
pub struct Clippy;

impl Clippy {
	pub fn run(self) -> Result<()> {
		let sh = crate::sh()?;

		for target in [
			Target::X86_64,
			Target::X86_64Fc,
			Target::X86_64Uefi,
			Target::Aarch64,
			Target::Riscv64,
		] {
			target.install()?;
			let triple = target.triple();
			let feature_flags = target.feature_flags();
			cmd!(sh, "cargo clippy --target={triple} {feature_flags...}").run()?;
		}

		cmd!(sh, "cargo clippy --package xtask").run()?;

		Ok(())
	}
}
