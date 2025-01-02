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
			cmd!(sh, "cargo clippy --target={triple} --features common-os").run()?;
			cmd!(sh, "cargo clippy --target={triple}")
				.arg("--features=acpi,dns,fsgsbase,pci,smp,vga")
				.run()?;
			cmd!(sh, "cargo clippy --target={triple}")
				.arg("--no-default-features")
				.run()?;
			cmd!(sh, "cargo clippy --target={triple}")
				.arg("--all-features")
				.run()?;
			cmd!(sh, "cargo clippy --target={triple}")
				.arg("--no-default-features")
				.arg("--features=tcp")
				.run()?;
			cmd!(sh, "cargo clippy --target={triple}")
				.arg("--no-default-features")
				.arg("--features=acpi,fsgsbase,pci,smp,vga")
				.run()?;

			match *arch {
				Arch::X86_64 => {
					cmd!(sh, "cargo clippy --target={triple}")
						.arg("--features=shell")
						.run()?;
				}
				Arch::Aarch64 => {}
				Arch::Riscv64 => {
					cmd!(sh, "cargo clippy --target={triple}")
						.arg("--no-default-features")
						.arg("--features=gem-net,tcp")
						.run()?;
				}
			}

			cmd!(sh, "cargo clippy --target={triple}")
				.arg("--no-default-features")
				.arg("--features=acpi,fsgsbase,newlib,smp,vga")
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
