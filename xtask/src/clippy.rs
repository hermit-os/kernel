use anyhow::Result;
use xshell::cmd;

use crate::arch::Arch;
use crate::flags;

impl flags::Clippy {
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
			// https://github.com/hermitcore/kernel/issues/470
			// cmd!(sh, "cargo clippy {target_args...}")
			// 	.arg("--no-default-features")
			// 	.arg("--features=acpi,fsgsbase,newlib,smp,vga")
			// 	.run()?;
		}

		cmd!(sh, "cargo clippy")
			.arg("--manifest-path=hermit-builtins/Cargo.toml")
			.arg("--target=x86_64-unknown-none")
			.run()?;

		cmd!(sh, "cargo clippy --package xtask").run()?;

		Ok(())
	}
}
