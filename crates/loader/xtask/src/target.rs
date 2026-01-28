use std::str::FromStr;

use anyhow::anyhow;
use xshell::{Shell, cmd};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Target {
	X86_64Linux,
	X86_64Multiboot,
	X86_64Uefi,
	Aarch64,
	Aarch64Be,
	Riscv64,
}

impl Target {
	pub fn install(&self) -> xshell::Result<()> {
		let sh = Shell::new()?;

		if self.tier() <= 2 {
			let triple = self.triple();
			cmd!(sh, "rustup target add {triple}").run()?;
		}

		if self == &Self::X86_64Multiboot {
			cmd!(sh, "rustup component add llvm-tools-preview").run()?;
		}

		Ok(())
	}

	pub fn arch(&self) -> &'static str {
		match self {
			Self::X86_64Linux => "x86_64",
			Self::X86_64Multiboot => "x86_64",
			Self::X86_64Uefi => "x86_64",
			Self::Aarch64 => "aarch64",
			Self::Aarch64Be => "aarch64_be",
			Self::Riscv64 => "riscv64",
		}
	}

	pub fn triple(&self) -> &'static str {
		match self {
			Self::X86_64Linux => "x86_64-unknown-none",
			Self::X86_64Multiboot => "x86_64-unknown-none",
			Self::X86_64Uefi => "x86_64-unknown-uefi",
			Self::Aarch64 => "aarch64-unknown-none-softfloat",
			Self::Aarch64Be => "aarch64_be-unknown-none-softfloat",
			Self::Riscv64 => "riscv64imac-unknown-none-elf",
		}
	}

	pub fn tier(&self) -> u8 {
		match self {
			Self::Aarch64Be => 3,
			_ => 2,
		}
	}

	pub fn cargo_args(&self) -> &'static [&'static str] {
		match self {
			Self::X86_64Linux => &["--target=x86_64-unknown-none"],
			Self::X86_64Multiboot => &["--target=x86_64-unknown-none"],
			Self::X86_64Uefi => &["--target=x86_64-unknown-uefi"],
			Self::Aarch64 => &["--target=aarch64-unknown-none-softfloat"],
			Self::Aarch64Be => &[
				"--target=aarch64_be-unknown-none-softfloat",
				"-Zbuild-std=core,alloc,panic_abort",
			],
			Self::Riscv64 => &["--target=riscv64imac-unknown-none-elf"],
		}
	}

	pub fn rustflags(&self) -> &'static [&'static str] {
		match self {
			Self::X86_64Linux => &[
				"-Clink-arg=-Tsrc/arch/x86_64/link_linux.ld",
				"-Crelocation-model=static",
			],
			Self::X86_64Multiboot => &[
				"-Clink-arg=-Tsrc/arch/x86_64/link_multiboot.ld",
				"-Crelocation-model=static",
			],
			Self::X86_64Uefi => &[],
			Self::Aarch64 | Self::Aarch64Be => &["-Clink-arg=-Tsrc/arch/aarch64/link.ld"],
			Self::Riscv64 => &["-Clink-arg=-Tsrc/arch/riscv64/link.ld"],
		}
	}

	pub fn feature_flags(&self) -> &'static [&'static str] {
		match self {
			Self::X86_64Linux => &["--features=linux"],
			Self::X86_64Multiboot => &["--features=multiboot"],
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
			Self::X86_64Linux => "hermit-loader-x86_64-linux",
			Self::X86_64Multiboot => "hermit-loader-x86_64-multiboot",
			Self::X86_64Uefi => "hermit-loader-x86_64.efi",
			Self::Aarch64 => "hermit-loader-aarch64",
			Self::Aarch64Be => "hermit-loader-aarch64_be",
			Self::Riscv64 => "hermit-loader-riscv64",
		}
	}

	pub fn qemu(&self) -> &'static str {
		match self {
			Self::X86_64Linux | Self::X86_64Multiboot | Self::X86_64Uefi => "x86_64",
			Self::Aarch64 | Self::Aarch64Be => "aarch64",
			Self::Riscv64 => "riscv64",
		}
	}
}

impl FromStr for Target {
	type Err = anyhow::Error;

	fn from_str(s: &str) -> Result<Self, Self::Err> {
		match s {
			"x86_64-linux" => Ok(Self::X86_64Linux),
			"x86_64-multiboot" => Ok(Self::X86_64Multiboot),
			"x86_64-uefi" => Ok(Self::X86_64Uefi),
			"aarch64" => Ok(Self::Aarch64),
			"aarch64_be" => Ok(Self::Aarch64Be),
			"riscv64" => Ok(Self::Riscv64),
			s => Err(anyhow!("Unsupported target: {s}")),
		}
	}
}
