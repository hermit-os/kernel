use std::str::FromStr;

use anyhow::anyhow;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Arch {
	X86_64,
	AArch64,
}

impl Arch {
	pub fn name(&self) -> &'static str {
		match self {
			Self::X86_64 => "x86_64",
			Self::AArch64 => "aarch64",
		}
	}

	pub fn triple(&self) -> &'static str {
		match self {
			Self::X86_64 => "x86_64-unknown-none",
			Self::AArch64 => "aarch64-unknown-none-softfloat",
		}
	}

	pub fn hermit_triple(&self) -> &'static str {
		match self {
			Arch::X86_64 => "x86_64-unknown-hermit",
			Arch::AArch64 => "aarch64-unknown-hermit",
		}
	}

	pub fn builtins_cargo_args(&self) -> &'static [&'static str] {
		match self {
			Arch::X86_64 => &[
				"--target=x86_64-unknown-hermit",
				"-Zbuild-std=core",
				"-Zbuild-std-features=compiler-builtins-mem",
			],
			Arch::AArch64 => &[
				"--target=aarch64-unknown-hermit",
				"-Zbuild-std=core",
				"-Zbuild-std-features=compiler-builtins-mem",
			],
		}
	}

	pub fn cargo_args(&self) -> &'static [&'static str] {
		match self {
			Self::X86_64 => &["--target=x86_64-unknown-none"],
			Self::AArch64 => &[
				"--target=aarch64-unknown-none-softfloat",
				// We can't use prebuilt std for aarch64 because it is built with
				// relocation-model=static and we need relocation-model=pic
				"-Zbuild-std=core,alloc",
				"-Zbuild-std-features=compiler-builtins-mem",
			],
		}
	}

	pub fn rustflags(&self) -> &'static [&'static str] {
		match self {
			Self::X86_64 => &[],
			Self::AArch64 => &["-Crelocation-model=pic"],
		}
	}
}

impl FromStr for Arch {
	type Err = anyhow::Error;

	fn from_str(s: &str) -> Result<Self, Self::Err> {
		match s {
			"x86_64" => Ok(Self::X86_64),
			"aarch64" => Ok(Self::AArch64),
			s => Err(anyhow!("Unsupported arch: {s}")),
		}
	}
}
