use anyhow::Result;
use clap::Args;

use crate::arch::Arch;

/// Run Clippy for all targets.
#[derive(Args)]
pub struct Clippy;

impl Clippy {
	pub fn run(self) -> Result<()> {
		for arch in Arch::all() {
			arch.install()?;

			let triple = arch.triple();
			let clippy = |args: &[&str]| {
				let mut cmd = crate::cargo();
				cmd.arg("clippy")
					.arg(format!("--target={triple}"))
					.arg("--all-targets")
					.args(args);
				eprintln!("$ {cmd:?}");
				cmd.spawn()?.wait()
			};

			clippy(&[])?;
			clippy(&["--features=common-os"])?;
			clippy(&["--features=acpi,dns,fsgsbase,pci,smp,vga"])?;
			clippy(&["--no-default-features"])?;
			clippy(&["--all-features"])?;
			clippy(&["--no-default-features", "--features=tcp"])?;
			clippy(&[
				"--no-default-features",
				"--features=acpi,fsgsbase,pci,smp,vga",
			])?;

			match *arch {
				Arch::X86_64 => {
					clippy(&["--features=shell"])?;
				}
				Arch::Aarch64 | Arch::Aarch64Be => {}
				Arch::Riscv64 => {
					clippy(&["--no-default-features", "--features=gem-net,tcp"])?;
				}
			}

			clippy(&[
				"--no-default-features",
				"--features=acpi,fsgsbase,newlib,smp,vga",
			])?;
		}

		let mut cmd = crate::cargo();
		cmd.args([
			"clippy",
			"--manifest-path=hermit-builtins/Cargo.toml",
			"--target=x86_64-unknown-none",
		]);
		eprintln!("$ {cmd:?}");
		cmd.spawn()?.wait()?;

		let mut cmd = crate::cargo();
		cmd.args(["clippy", "--package", "xtask"]);
		eprintln!("$ {cmd:?}");
		cmd.spawn()?.wait()?;

		Ok(())
	}
}
