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

		for target in [Arch::X86_64, Arch::Aarch64] {
			let target_args = target.cargo_args();
			cmd!(sh, "cargo clippy {target_args...}").run()?;
			cmd!(sh, "cargo clippy {target_args...}")
				.arg("--no-default-features")
				.run()?;
			cmd!(sh, "cargo clippy {target_args...}")
				.arg("--no-default-features")
				.arg("--features=acpi,fsgsbase,pci,smp,vga")
				.run()?;
			// TODO: Enable clippy for newlib
			// https://github.com/hermit-os/kernel/issues/470
			// cmd!(sh, "cargo clippy {target_args...}")
			// 	.arg("--no-default-features")
			// 	.arg("--features=acpi,fsgsbase,newlib,smp,vga")
			// 	.run()?;
		}

		{
			let target_args = Arch::Riscv64.cargo_args();
			cmd!(sh, "cargo clippy {target_args...}")
				.arg("--no-default-features")
				.run()?;
			cmd!(sh, "cargo clippy {target_args...}")
				.arg("--no-default-features")
				.arg("--features=smp,tcp")
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
