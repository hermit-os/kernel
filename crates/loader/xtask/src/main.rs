//! See <https://github.com/matklad/cargo-xtask/>.

mod flags;
mod rustc;

use std::{
	env::{self, VarError},
	ffi::OsString,
	path::{Path, PathBuf},
};

use anyhow::{anyhow, Result};
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
			.args(self.release_args())
			.args(self.profile_args())
			.run()?;

		eprintln!("Converting binary");
		self.convert_binary()?;

		eprintln!("Loader available at {}", self.dist_binary().display());
		Ok(())
	}

	fn cargo_encoded_rustflags(&self) -> Result<String> {
		let outer_rustflags = match env::var("CARGO_ENCODED_RUSTFLAGS") {
			Ok(s) => Some(s),
			Err(VarError::NotPresent) => None,
			Err(err) => return Err(err.into()),
		};

		let mut rustflags = outer_rustflags.map(|s| vec![s]).unwrap_or_default();
		let arch = self.arch.as_str();
		rustflags.push(format!("-Clink-arg=-Tsrc/arch/{arch}/link.ld"));
		Ok(rustflags.join("\x1f"))
	}

	fn target_dir_args(&self) -> Vec<OsString> {
		vec!["--target-dir".into(), self.target_dir().into()]
	}

	fn release_args(&self) -> &[&str] {
		if self.release {
			&["--release"]
		} else {
			&[]
		}
	}

	fn profile_args(&self) -> Vec<&str> {
		match self.profile.as_deref() {
			Some(profile) => vec!["--profile", profile],
			None => vec![],
		}
	}

	fn convert_binary(&self) -> Result<()> {
		let sh = sh()?;

		let input = self.build_binary();
		let output = self.dist_binary();
		sh.create_dir(output.parent().unwrap())?;
		sh.copy_file(&input, &output)?;

		let objcopy = binutil("objcopy")?;

		if self.arch == "x86_64" {
			cmd!(sh, "{objcopy} -O elf32-i386 {output}").run()?;
		}

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
			"dev" => "debug",
			profile => profile,
		});
		out_dir
	}

	fn build_binary(&self) -> PathBuf {
		let mut build_binary = self.out_dir();
		build_binary.push("rusty-loader");
		build_binary
	}

	fn dist_binary(&self) -> PathBuf {
		let mut dist_binary = self.dist_dir();
		dist_binary.push("rusty-loader");
		dist_binary
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
		"x86_64" => Ok("x86_64-unknown-hermit-loader"),
		"aarch64" => Ok("aarch64-unknown-hermit-loader"),
		arch => Err(anyhow!("Unsupported arch: {arch}")),
	}
}

fn target_args(arch: &str) -> Result<&'static [&'static str]> {
	match arch {
		"x86_64" => Ok(&[
			"--target=targets/x86_64-unknown-hermit-loader.json",
			"-Zbuild-std=core,alloc",
			"-Zbuild-std-features=compiler-builtins-mem",
		]),
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

	let mut path = rustc::rustlib()?;
	path.push(exe);
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
