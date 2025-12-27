use std::ffi::OsStr;
use std::io;

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
			let target = format!("--target={triple}");
			let target = target.as_str();

			clippy([target])?;
			clippy([target, "--features=common-os"])?;
			clippy([target, "--features=acpi,dns,fsgsbase,pci,smp,vga"])?;
			clippy([target, "--no-default-features"])?;
			clippy([target, "--all-features"])?;
			clippy([target, "--no-default-features", "--features=tcp"])?;
			clippy([
				target,
				"--no-default-features",
				"--features=acpi,fsgsbase,pci,smp,vga",
			])?;

			match *arch {
				Arch::X86_64 => clippy([target, "--features=shell"])?,
				Arch::Aarch64 | Arch::Aarch64Be => {}
				Arch::Riscv64 => {
					clippy([target, "--no-default-features", "--features=gem-net,tcp"])?;
				}
			}

			clippy([
				target,
				"--no-default-features",
				"--features=acpi,fsgsbase,newlib,smp,vga",
			])?;
		}

		clippy([
			"--manifest-path=hermit-builtins/Cargo.toml",
			"--target=x86_64-unknown-none",
		])?;

		clippy(["--package=xtask"])?;

		Ok(())
	}
}

fn clippy<I, S>(args: I) -> io::Result<()>
where
	I: IntoIterator<Item = S>,
	S: AsRef<OsStr>,
{
	let mut cmd = crate::cargo();
	cmd.arg("clippy").arg("--all-targets").args(args);

	eprintln!("$ {cmd:?}");
	cmd.spawn()?.wait()?;

	Ok(())
}
