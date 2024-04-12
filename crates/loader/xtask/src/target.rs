use std::str::FromStr;

use anyhow::anyhow;
use xshell::{cmd, Shell};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Target {
	X86_64,
	X86_64Fc,
	X86_64Uefi,
	Aarch64,
	Riscv64,
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

	pub fn arch(&self) -> &'static str {
		match self {
			Self::X86_64 => "x86_64",
			Self::X86_64Fc => "x86_64",
			Self::X86_64Uefi => "x86_64",
			Self::Aarch64 => "aarch64",
			Self::Riscv64 => "riscv64",
		}
	}

	pub fn triple(&self) -> &'static str {
		match self {
			Self::X86_64 => "x86_64-unknown-none",
			Self::X86_64Fc => "x86_64-unknown-none",
			Self::X86_64Uefi => "x86_64-unknown-uefi",
			Self::Aarch64 => "aarch64-unknown-none-softfloat",
			Self::Riscv64 => "riscv64imac-unknown-none-elf",
		}
	}

	pub fn cargo_args(&self) -> &'static [&'static str] {
		match self {
			Self::X86_64 => &["--target=x86_64-unknown-none"],
			Self::X86_64Fc => &["--target=x86_64-unknown-none"],
			Self::X86_64Uefi => &["--target=x86_64-unknown-uefi"],
			Self::Aarch64 => &["--target=aarch64-unknown-none-softfloat"],
			Self::Riscv64 => &["--target=riscv64imac-unknown-none-elf"],
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
			Self::Aarch64 => &["-Clink-arg=-Tsrc/arch/aarch64/link.ld"],
			Self::Riscv64 => &["-Clink-arg=-Tsrc/arch/riscv64/link.ld"],
		}
	}

	pub fn feature_flags(&self) -> &'static [&'static str] {
		match self {
			Self::X86_64Fc => &["--features=fc"],
			_ => &[],
		}
	}

	pub fn image_name(&self) -> &'static str {
		match self {
			Self::X86_64Uefi => "hermit-loader.efi",
			_ => "hermit-loader",
		}
	}

	pub fn dist_name(&self) -> &'static str {
		match self {
			Self::X86_64 => "hermit-loader-x86_64",
			Self::X86_64Fc => "hermit-loader-x86_64-fc",
			Self::X86_64Uefi => "hermit-loader-x86_64.efi",
			Self::Aarch64 => "hermit-loader-aarch64",
			Self::Riscv64 => "hermit-loader-riscv64",
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
			"aarch64" => Ok(Self::Aarch64),
			"riscv64" => Ok(Self::Riscv64),
			s => Err(anyhow!("Unsupported target: {s}")),
		}
	}
}
