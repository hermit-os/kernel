use anyhow::Result;
use clap::ValueEnum;
use xshell::cmd;

/// Target architecture.
#[derive(ValueEnum, Clone, Copy, PartialEq, Eq, Debug)]
#[value(rename_all = "snake_case")]
pub enum Arch {
	/// x86-64
	X86_64,
	/// AArch64
	Aarch64,
	/// 64-bit RISC-V
	Riscv64,
}

impl Arch {
	pub fn all() -> &'static [Self] {
		&[Self::X86_64, Self::Aarch64, Self::Riscv64]
	}

	pub fn install(&self) -> Result<()> {
		let sh = crate::sh()?;
		let triple = self.triple();
		cmd!(sh, "rustup target add {triple}").run()?;
		Ok(())
	}

	pub fn install_for_build(&self) -> Result<()> {
		if self == &Self::X86_64 {
			self.install()?;
		}
		Ok(())
	}

	pub fn name(&self) -> &'static str {
		match self {
			Self::X86_64 => "x86_64",
			Self::Aarch64 => "aarch64",
			Self::Riscv64 => "riscv64",
		}
	}

	pub fn triple(&self) -> &'static str {
		match self {
			Self::X86_64 => "x86_64-unknown-none",
			Self::Aarch64 => "aarch64-unknown-none-softfloat",
			Self::Riscv64 => "riscv64gc-unknown-none-elf",
		}
	}

	pub fn hermit_triple(&self) -> &'static str {
		match self {
			Self::X86_64 => "x86_64-unknown-hermit",
			Self::Aarch64 => "aarch64-unknown-hermit",
			Self::Riscv64 => "riscv64gc-unknown-hermit",
		}
	}

	pub fn builtins_cargo_args(&self) -> &'static [&'static str] {
		match self {
			Self::X86_64 => &[
				"--target=x86_64-unknown-hermit",
				"-Zbuild-std=core",
				"-Zbuild-std-features=compiler-builtins-mem",
			],
			Self::Aarch64 => &[
				"--target=aarch64-unknown-hermit",
				"-Zbuild-std=core",
				"-Zbuild-std-features=compiler-builtins-mem",
			],
			Arch::Riscv64 => &[
				"--target=riscv64gc-unknown-hermit",
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
				// We can't use prebuilt std here because it is built with
				// relocation-model=static and we need relocation-model=pic
				"-Zbuild-std=core,alloc",
				"-Zbuild-std-features=compiler-builtins-mem",
			],
			Self::Riscv64 => &[
				"--target=riscv64gc-unknown-none-elf",
				// We can't use prebuilt std here because it is built with
				// relocation-model=static and we need relocation-model=pic
				"-Zbuild-std=core,alloc",
				"-Zbuild-std-features=compiler-builtins-mem",
			],
		}
	}

	pub fn ci_cargo_args(&self) -> &'static [&'static str] {
		match self {
			Self::X86_64 => &[
				"--target=x86_64-unknown-hermit",
				"-Zbuild-std=std,panic_abort",
			],
			Self::Aarch64 => &[
				"--target=aarch64-unknown-hermit",
				"-Zbuild-std=std,panic_abort",
			],
			Arch::Riscv64 => &[
				"--target=riscv64gc-unknown-hermit",
				"-Zbuild-std=std,panic_abort",
			],
		}
	}

	pub fn rustflags(&self) -> &'static [&'static str] {
		match self {
			Self::X86_64 => &[],
			Self::Aarch64 => &["-Crelocation-model=pic"],
			Self::Riscv64 => &["-Cno-redzone", "-Crelocation-model=pic"],
		}
	}
}
