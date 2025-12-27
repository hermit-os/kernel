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
			let clippy = || cmd!(sh, "cargo clippy --target={triple} --all-targets");

			clippy().run()?;
			clippy().arg("--features=common-os").run()?;
			clippy()
				.arg("--features=acpi,dns,fsgsbase,pci,smp,vga")
				.run()?;
			clippy().arg("--no-default-features").run()?;
			clippy().arg("--all-features").run()?;
			clippy()
				.arg("--no-default-features")
				.arg("--features=tcp")
				.run()?;
			clippy()
				.arg("--no-default-features")
				.arg("--features=acpi,fsgsbase,pci,smp,vga")
				.run()?;
			clippy()
				.arg("--no-default-features")
				// FIXME: also enable virtio-fs and virtio-vsock once they no longer imply PCI
				.arg("--features=tcp,virtio-console,virtio-net")
				.run()?;

			match *arch {
				Arch::X86_64 => {
					clippy().arg("--features=shell").run()?;
				}
				Arch::Aarch64 | Arch::Aarch64Be => {}
				Arch::Riscv64 => {
					clippy()
						.arg("--no-default-features")
						.arg("--features=gem-net,tcp")
						.run()?;
				}
			}

			clippy()
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
