//! See <https://github.com/matklad/cargo-xtask/>.

mod arch;
mod flags;

use std::{
	env::{self, VarError},
	ffi::OsStr,
	path::{Path, PathBuf},
};

use anyhow::{anyhow, Result};
use llvm_tools::LlvmTools;
use xshell::{cmd, Shell};

use crate::arch::Arch;

fn main() -> Result<()> {
	flags::Xtask::from_env()?.run()
}

impl flags::Xtask {
	fn run(self) -> Result<()> {
		match self.subcommand {
			flags::XtaskCmd::Help(_) => {
				println!("{}", flags::Xtask::HELP);
				Ok(())
			}
			flags::XtaskCmd::Build(build) => build.run(),
			flags::XtaskCmd::Clippy(clippy) => clippy.run(),
		}
	}
}

impl flags::Build {
	fn run(self) -> Result<()> {
		let sh = sh()?;

		eprintln!("Building loader");
		cmd!(sh, "cargo build")
			.env("CARGO_ENCODED_RUSTFLAGS", self.cargo_encoded_rustflags()?)
			.args(self.arch.cargo_args())
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

		if self.arch == Arch::X86_64 {
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

		rustflags.extend(self.arch.rustflags());

		Ok(rustflags.join("\x1f"))
	}

	fn target_dir_args(&self) -> [&OsStr; 2] {
		["--target-dir".as_ref(), self.target_dir().as_ref()]
	}

	fn profile_args(&self) -> [&str; 2] {
		["--profile", self.profile()]
	}

	fn convert_to_elf32_i386(&self) -> Result<()> {
		let sh = sh()?;
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
		out_dir.push(self.arch.triple());
		out_dir.push(match self.profile() {
			"dev" => "debug",
			profile => profile,
		});
		out_dir
	}

	fn dist_dir(&self) -> PathBuf {
		let mut out_dir = self.target_dir().to_path_buf();
		out_dir.push(self.arch.name());
		out_dir.push(match self.profile() {
			"dev" => "debug",
			profile => profile,
		});
		out_dir
	}

	fn build_object(&self) -> PathBuf {
		let mut build_object = self.out_dir();
		build_object.push(self.arch.build_name());
		build_object
	}

	fn dist_object(&self) -> PathBuf {
		let mut dist_object = self.dist_dir();
		dist_object.push(self.arch.dist_name());
		dist_object
	}
}

impl flags::Clippy {
	fn run(self) -> Result<()> {
		let sh = sh()?;

		// TODO: Enable clippy for aarch64
		// https://github.com/hermitcore/rusty-loader/issues/78
		// TODO: Enable clippy for x86_64-uefi
		// https://github.com/hermitcore/rusty-loader/issues/122
		#[allow(clippy::single_element_loop)]
		for arch in [Arch::X86_64] {
			let target_args = arch.cargo_args();
			cmd!(sh, "cargo clippy {target_args...}")
				.env("HERMIT_APP", hermit_app(arch))
				.run()?;
		}

		cmd!(sh, "cargo clippy --package xtask").run()?;

		Ok(())
	}
}

fn hermit_app(arch: Arch) -> PathBuf {
	let mut hermit_app = project_root().to_path_buf();
	hermit_app.push("data");
	hermit_app.push(arch.name());
	hermit_app.push("hello_world");
	hermit_app
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

fn sh() -> Result<Shell> {
	let sh = Shell::new()?;
	sh.change_dir(project_root());
	Ok(sh)
}

fn project_root() -> &'static Path {
	Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()
}
