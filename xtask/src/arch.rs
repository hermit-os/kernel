use clap::ValueEnum;

/// Target architecture.
#[derive(ValueEnum, Clone, Copy, PartialEq, Eq, Debug)]
#[value(rename_all = "snake_case")]
pub enum Arch {
	/// x86-64
	X86_64,
	/// AArch64
	Aarch64,
}

impl Arch {
	pub fn name(&self) -> &'static str {
		match self {
			Self::X86_64 => "x86_64",
			Self::Aarch64 => "aarch64",
		}
	}

	pub fn triple(&self) -> &'static str {
		match self {
			Self::X86_64 => "x86_64-unknown-none",
			Self::Aarch64 => "aarch64-unknown-none-softfloat",
		}
	}

	pub fn hermit_triple(&self) -> &'static str {
		match self {
			Arch::X86_64 => "x86_64-unknown-hermit",
			Arch::Aarch64 => "aarch64-unknown-hermit",
		}
	}

	pub fn builtins_cargo_args(&self) -> &'static [&'static str] {
		match self {
			Arch::X86_64 => &[
				"--target=x86_64-unknown-hermit",
				"-Zbuild-std=core",
				"-Zbuild-std-features=compiler-builtins-mem",
			],
			Arch::Aarch64 => &[
				"--target=aarch64-unknown-hermit",
				"-Zbuild-std=core",
				"-Zbuild-std-features=compiler-builtins-mem",
			],
		}
	}

	pub fn cargo_args(&self) -> &'static [&'static str] {
		match self {
			Self::X86_64 => &["--target=x86_64-unknown-none"],
			Self::Aarch64 => &[
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
			Self::Aarch64 => &["-Crelocation-model=pic"],
		}
	}
}
