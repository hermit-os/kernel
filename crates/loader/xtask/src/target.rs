use std::str::FromStr;

use anyhow::anyhow;
use xshell::{cmd, Shell};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Target {
	X86_64,
	X86_64Fc,
	X86_64Uefi,
	AArch64,
}

impl Target {
	pub fn install(&self) -> xshell::Result<()> {
		let sh = Shell::new()?;

		let triple = self.triple();
		cmd!(sh, "rustup target add {triple}").run()?;

		if self == &Self::X86_64 {
			cmd!(sh, "rustup component add llvm-tools-preview").run()?;
		}

		Ok(())
	}

	pub fn name(&self) -> &'static str {
		match self {
			Self::X86_64 => "x86_64",
			Self::X86_64Fc => "x86_64-fc",
			Self::X86_64Uefi => "x86_64-uefi",
			Self::AArch64 => "aarch64",
		}
	}

	pub fn triple(&self) -> &'static str {
		match self {
			Self::X86_64 => "x86_64-unknown-none",
			Self::X86_64Fc => "x86_64-unknown-none",
			Self::X86_64Uefi => "x86_64-unknown-uefi",
			Self::AArch64 => "aarch64-unknown-none-softfloat",
		}
	}

	pub fn rustflags(&self) -> &'static [&'static str] {
		match self {
			Self::X86_64 => &[
				"-Clink-arg=-Tsrc/arch/x86_64/link.ld",
				"-Crelocation-model=static",
			],
			Self::X86_64Fc => &[
				"-Clink-arg=-Tsrc/arch/x86_64/link_fc.ld",
				"-Crelocation-model=static",
			],
			Self::X86_64Uefi => &[],
			Self::AArch64 => &["-Clink-arg=-Tsrc/arch/aarch64/link.ld"],
		}
	}

	pub fn build_name(&self) -> &'static str {
		match self {
			Self::X86_64Uefi => "rusty-loader.efi",
			_ => "rusty-loader",
		}
	}

	pub fn dist_name(&self) -> &'static str {
		match self {
			Self::X86_64Uefi => "BootX64.efi",
			_ => "rusty-loader",
		}
	}
}

impl FromStr for Target {
	type Err = anyhow::Error;

	fn from_str(s: &str) -> Result<Self, Self::Err> {
		match s {
			"x86_64" => Ok(Self::X86_64),
			"x86_64-fc" => Ok(Self::X86_64Fc),
			"x86_64-uefi" => Ok(Self::X86_64Uefi),
			"aarch64" => Ok(Self::AArch64),
			s => Err(anyhow!("Unsupported target: {s}")),
		}
	}
}
