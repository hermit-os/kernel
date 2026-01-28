use std::str::FromStr;

use anyhow::anyhow;
use xshell::{Shell, cmd};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Target {
	X86_64Linux,
	X86_64Multiboot,
	X86_64Uefi,
	Aarch64Elf,
	Aarch64BeElf,
	Riscv64Sbi,
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
			Self::Aarch64Elf => "aarch64",
			Self::Aarch64BeElf => "aarch64_be",
			Self::Riscv64Sbi => "riscv64",
		}
	}

	pub fn triple(&self) -> &'static str {
		match self {
			Self::X86_64Linux => "x86_64-unknown-none",
			Self::X86_64Multiboot => "x86_64-unknown-none",
			Self::X86_64Uefi => "x86_64-unknown-uefi",
			Self::Aarch64Elf => "aarch64-unknown-none-softfloat",
			Self::Aarch64BeElf => "aarch64_be-unknown-none-softfloat",
			Self::Riscv64Sbi => "riscv64imac-unknown-none-elf",
		}
	}

	pub fn tier(&self) -> u8 {
		match self {
			Self::Aarch64BeElf => 3,
			_ => 2,
		}
	}

	pub fn cargo_args(&self) -> &'static [&'static str] {
		match self {
			Self::X86_64Linux => &["--target=x86_64-unknown-none"],
			Self::X86_64Multiboot => &["--target=x86_64-unknown-none"],
			Self::X86_64Uefi => &["--target=x86_64-unknown-uefi"],
			Self::Aarch64Elf => &["--target=aarch64-unknown-none-softfloat"],
			Self::Aarch64BeElf => &[
				"--target=aarch64_be-unknown-none-softfloat",
				"-Zbuild-std=core,alloc,panic_abort",
			],
			Self::Riscv64Sbi => &["--target=riscv64imac-unknown-none-elf"],
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
			Self::Aarch64Elf | Self::Aarch64BeElf => &["-Clink-arg=-Tsrc/arch/aarch64/link.ld"],
			Self::Riscv64Sbi => &["-Clink-arg=-Tsrc/arch/riscv64/link.ld"],
		}
	}

	pub fn feature_flags(&self) -> &'static [&'static str] {
		match self {
			Self::X86_64Linux => &["--features=x86_64-linux"],
			Self::X86_64Multiboot => &["--features=x86_64-multiboot"],
			Self::Aarch64Elf | Self::Aarch64BeElf => &["--features=elf"],
			Self::Riscv64Sbi => &["--features=sbi"],
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
			Self::Aarch64Elf => "hermit-loader-aarch64-elf",
			Self::Aarch64BeElf => "hermit-loader-aarch64_be-elf",
			Self::Riscv64Sbi => "hermit-loader-riscv64-sbi",
		}
	}

	pub fn qemu(&self) -> &'static str {
		match self {
			Self::X86_64Linux | Self::X86_64Multiboot | Self::X86_64Uefi => "x86_64",
			Self::Aarch64Elf | Self::Aarch64BeElf => "aarch64",
			Self::Riscv64Sbi => "riscv64",
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
			"aarch64-elf" => Ok(Self::Aarch64Elf),
			"aarch64_be-elf" => Ok(Self::Aarch64BeElf),
			"riscv64-sbi" => Ok(Self::Riscv64Sbi),
			s => Err(anyhow!("Unsupported target: {s}")),
		}
	}
}
