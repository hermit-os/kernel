use anyhow::Result;
use clap::Args;
use xshell::cmd;

use crate::arch::Arch;

/// Run Clippy for all targets.
#[derive(Args)]
pub struct Clippy;

impl Clippy {
	pub fn run(self) -> Result<()> {
		let sh = crate::sh()?;

		for arch in Arch::all() {
			arch.install()?;

			let triple = arch.triple();
			cmd!(sh, "cargo clippy --target={triple}").run()?;
			cmd!(sh, "cargo clippy --target={triple}")
				.arg("--no-default-features")
				.run()?;
			cmd!(sh, "cargo clippy --target={triple}")
				.arg("--no-default-features")
				.arg("--features=tcp")
				.run()?;
			cmd!(sh, "cargo clippy --target={triple}")
				.arg("--no-default-features")
				.arg("--features=acpi,fsgsbase,pci,shell,smp,vga")
				.run()?;

			if *arch == Arch::Riscv64 {
				cmd!(sh, "cargo clippy --target={triple}")
					.arg("--no-default-features")
					.arg("--features=gem-net,tcp")
					.run()?;
			}

			cmd!(sh, "cargo clippy --target={triple}")
				.arg("--no-default-features")
				.arg("--features=acpi,fsgsbase,newlib,shell,smp,vga")
				.run()?;
		}

		cmd!(sh, "cargo clippy")
			.arg("--manifest-path=hermit-builtins/Cargo.toml")
			.arg("--target=x86_64-unknown-none")
			.run()?;

		cmd!(sh, "cargo clippy --package xtask").run()?;

		Ok(())
	}
}
