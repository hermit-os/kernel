//! See <https://github.com/matklad/cargo-xtask/>.

mod flags;

use std::{
	env::{self, VarError},
	ffi::OsString,
	path::{Path, PathBuf},
};

use anyhow::{anyhow, bail, Result};
use llvm_tools::LlvmTools;
use xshell::{cmd, Shell};

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
			.args(target_args(&self.arch)?)
			.args(self.target_dir_args())
			.args(self.profile_args())
			.run()?;

		let build_object = self.build_object();
		let dist_object = self.dist_object();
		sh.create_dir(dist_object.parent().unwrap())?;
		sh.copy_file(&build_object, &dist_object)?;

		if self.arch == "x86_64" {
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

		match self.arch.as_str() {
			"x86_64" => {
				rustflags.push("-Clink-arg=-Tsrc/arch/x86_64/link.ld");
				rustflags.push("-Crelocation-model=static");
			}
			"aarch64" => {
				rustflags.push("-Clink-arg=-Tsrc/arch/aarch64/link.ld");
			}
			arch => bail!("Unsupported arch: {arch}"),
		};

		// TODO: Use cargo's `opt-level = 0` instead of this:
		// https://github.com/hermitcore/rusty-loader/issues/45
		if self.profile() == "x86_64-dev" {
			rustflags.push("-Cdebug-assertions=y");
			rustflags.push("-Clto=n");
		}

		Ok(rustflags.join("\x1f"))
	}

	fn target_dir_args(&self) -> Vec<OsString> {
		vec!["--target-dir".into(), self.target_dir().into()]
	}

	fn profile_args(&self) -> Vec<&str> {
		vec!["--profile", self.profile()]
	}

	fn convert_to_elf32_i386(&self) -> Result<()> {
		let sh = sh()?;
		let objcopy = binutil("objcopy")?;
		let object = self.dist_object();
		cmd!(sh, "{objcopy} --output-target elf32-i386 {object}").run()?;
		Ok(())
	}

	fn profile(&self) -> &str {
		let profile =
			self.profile
				.as_deref()
				.unwrap_or(if self.release { "release" } else { "dev" });

		// TODO: Use cargo's `opt-level = 0` instead of this:
		// https://github.com/hermitcore/rusty-loader/issues/45
		match profile {
			"dev" if self.arch == "x86_64" => "x86_64-dev",
			profile => profile,
		}
	}

	fn target_dir(&self) -> &Path {
		self.target_dir
			.as_deref()
			.unwrap_or_else(|| Path::new("target"))
	}

	fn out_dir(&self) -> PathBuf {
		let mut out_dir = self.target_dir().to_path_buf();
		out_dir.push(target(&self.arch).unwrap());
		out_dir.push(match self.profile() {
			"dev" => "debug",
			profile => profile,
		});
		out_dir
	}

	fn dist_dir(&self) -> PathBuf {
		let mut out_dir = self.target_dir().to_path_buf();
		out_dir.push(&self.arch);
		out_dir.push(match self.profile() {
			"dev" | "x86_64-dev" => "debug",
			profile => profile,
		});
		out_dir
	}

	fn build_object(&self) -> PathBuf {
		let mut build_object = self.out_dir();
		build_object.push("rusty-loader");
		build_object
	}

	fn dist_object(&self) -> PathBuf {
		let mut dist_object = self.dist_dir();
		dist_object.push("rusty-loader");
		dist_object
	}
}

impl flags::Clippy {
	fn run(self) -> Result<()> {
		let sh = sh()?;

		// TODO: Enable clippy for aarch64
		// https://github.com/hermitcore/rusty-loader/issues/78
		#[allow(clippy::single_element_loop)]
		for target in ["x86_64"] {
			let target_arg = target_args(target)?;
			let hermit_app = {
				let mut hermit_app = project_root().to_path_buf();
				hermit_app.push("data");
				hermit_app.push(target);
				hermit_app.push("hello_world");
				hermit_app
			};
			cmd!(sh, "cargo clippy {target_arg...}")
				.env("HERMIT_APP", &hermit_app)
				.run()?;
		}

		cmd!(sh, "cargo clippy --package xtask").run()?;

		Ok(())
	}
}

fn target(arch: &str) -> Result<&'static str> {
	match arch {
		"x86_64" => Ok("x86_64-unknown-none"),
		"aarch64" => Ok("aarch64-unknown-hermit-loader"),
		arch => Err(anyhow!("Unsupported arch: {arch}")),
	}
}

fn target_args(arch: &str) -> Result<&'static [&'static str]> {
	match arch {
		"x86_64" => Ok(&["--target=x86_64-unknown-none"]),
		"aarch64" => Ok(&[
			"--target=targets/aarch64-unknown-hermit-loader.json",
			"-Zbuild-std=core,alloc",
			"-Zbuild-std-features=compiler-builtins-mem",
		]),
		arch => Err(anyhow!("Unsupported arch: {arch}")),
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

fn sh() -> Result<Shell> {
	let sh = Shell::new()?;
	sh.change_dir(project_root());
	Ok(sh)
}

fn project_root() -> &'static Path {
	Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()
}
