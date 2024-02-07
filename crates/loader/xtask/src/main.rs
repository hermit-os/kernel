//! See <https://github.com/matklad/cargo-xtask/>.

mod flags;
mod target;

use std::env::{self, VarError};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};
use llvm_tools::LlvmTools;
use xshell::{cmd, Shell};

use crate::target::Target;

fn main() -> Result<()> {
	flags::Xtask::from_env()?.run()
}

impl flags::Xtask {
	fn run(self) -> Result<()> {
		match self.subcommand {
			flags::XtaskCmd::Build(build) => build.run(),
			flags::XtaskCmd::Clippy(clippy) => clippy.run(),
		}
	}
}

impl flags::Build {
	fn run(self) -> Result<()> {
		self.target.install()?;

		let sh = Shell::new()?;

		eprintln!("Building loader");
		let triple = self.target.triple();
		cmd!(sh, "cargo build --target={triple}")
			.env("CARGO_ENCODED_RUSTFLAGS", self.cargo_encoded_rustflags()?)
			.args(self.target.feature_flags())
			.args(self.target_dir_args())
			.args(self.profile_args())
			.run()?;

		let build_object = self.build_object();
		let dist_object = self.dist_object();
		eprintln!(
			"Copying {} to {}",
			build_object.display(),
			dist_object.display()
		);
		sh.create_dir(dist_object.parent().unwrap())?;
		sh.copy_file(&build_object, &dist_object)?;

		if self.target == Target::X86_64 {
			eprintln!("Converting object to elf32-i386");
			self.convert_to_elf32_i386()?;
		}

		eprintln!("Loader available at {}", self.dist_object().display());
		Ok(())
	}

	fn cargo_encoded_rustflags(&self) -> Result<String> {
		let outer_rustflags = match env::var("CARGO_ENCODED_RUSTFLAGS") {
			Ok(s) => Some(s),
			Err(VarError::NotPresent) => None,
			Err(err) => return Err(err.into()),
		};

		let mut rustflags = outer_rustflags
			.as_ref()
			.map(|s| vec![s.as_str()])
			.unwrap_or_default();

		rustflags.extend(self.target.rustflags());

		Ok(rustflags.join("\x1f"))
	}

	fn target_dir_args(&self) -> [&OsStr; 2] {
		["--target-dir".as_ref(), self.target_dir().as_ref()]
	}

	fn profile_args(&self) -> [&str; 2] {
		["--profile", self.profile()]
	}

	fn convert_to_elf32_i386(&self) -> Result<()> {
		let sh = Shell::new()?;
		let objcopy = binutil("objcopy")?;
		let object = self.dist_object();
		cmd!(sh, "{objcopy} --output-target elf32-i386 {object}").run()?;
		Ok(())
	}

	fn profile(&self) -> &str {
		self.profile
			.as_deref()
			.unwrap_or(if self.release { "release" } else { "dev" })
	}

	fn target_dir(&self) -> &Path {
		self.target_dir
			.as_deref()
			.unwrap_or_else(|| Path::new("target"))
	}

	fn out_dir(&self) -> PathBuf {
		let mut out_dir = self.target_dir().to_path_buf();
		out_dir.push(self.target.triple());
		out_dir.push(match self.profile() {
			"dev" => "debug",
			profile => profile,
		});
		out_dir
	}

	fn dist_dir(&self) -> PathBuf {
		let mut out_dir = self.target_dir().to_path_buf();
		out_dir.push(self.target.name());
		out_dir.push(match self.profile() {
			"dev" => "debug",
			profile => profile,
		});
		out_dir
	}

	fn build_object(&self) -> PathBuf {
		let mut build_object = self.out_dir();
		build_object.push(self.target.build_name());
		build_object
	}

	fn dist_object(&self) -> PathBuf {
		let mut dist_object = self.dist_dir();
		dist_object.push(self.target.dist_name());
		dist_object
	}
}

impl flags::Clippy {
	fn run(self) -> Result<()> {
		let sh = Shell::new()?;

		// TODO: Enable clippy for x86_64-uefi
		// https://github.com/hermitcore/loader/issues/122
		for target in [
			Target::X86_64,
			Target::X86_64Fc,
			Target::AArch64,
			Target::Riscv64,
		] {
			target.install()?;
			let triple = target.triple();
			let feature_flags = target.feature_flags();
			cmd!(sh, "cargo clippy --target={triple} {feature_flags...}").run()?;
		}

		cmd!(sh, "cargo clippy --package xtask").run()?;

		Ok(())
	}
}

fn binutil(name: &str) -> Result<PathBuf> {
	let exe_suffix = env::consts::EXE_SUFFIX;
	let exe = format!("llvm-{name}{exe_suffix}");

	let path = LlvmTools::new()
		.map_err(|err| anyhow!("{err:?}"))?
		.tool(&exe)
		.ok_or_else(|| anyhow!("could not find {exe}"))?;

	Ok(path)
}
