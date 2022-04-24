//! See <https://github.com/matklad/cargo-xtask/>.

mod flags;
mod rustc;

use std::{
	env::{self, VarError},
	ffi::OsString,
	path::{Path, PathBuf},
};

use anyhow::{anyhow, Result};
use goblin::archive::Archive;
use xshell::{cmd, Shell};

const RUSTFLAGS: &[&str] = &[
	// TODO: Re-enable mutable-noalias
	// https://github.com/hermitcore/libhermit-rs/issues/200
	"-Zmutable-noalias=no",
];

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

		eprintln!("Building kernel");
		cmd!(sh, "cargo build")
			.env("CARGO_ENCODED_RUSTFLAGS", self.cargo_encoded_rustflags()?)
			.args(target_args(&self.arch)?)
			.args(self.target_dir_args())
			.args(self.no_default_features_args())
			.args(self.features_args())
			.args(self.release_args())
			.args(self.profile_args())
			.run()?;

		eprintln!("Exporting symbols");
		self.export_syms()?;

		eprintln!("Kernel available at {}", self.dist_archive().display());
		Ok(())
	}

	fn cargo_encoded_rustflags(&self) -> Result<String> {
		let outer_rustflags = match env::var("CARGO_ENCODED_RUSTFLAGS") {
			Ok(s) => Some(s),
			Err(VarError::NotPresent) => None,
			Err(err) => return Err(err.into()),
		};

		let mut rustflags = outer_rustflags
			.as_deref()
			.map(|s| vec![s])
			.unwrap_or_default();
		rustflags.extend(RUSTFLAGS);
		if self.instrument_mcount {
			rustflags.push("-Zinstrument-mcount");
		}
		Ok(rustflags.join("\x1f"))
	}

	fn target_dir_args(&self) -> Vec<OsString> {
		vec!["--target-dir".into(), self.target_dir().into()]
	}

	fn no_default_features_args(&self) -> &[&str] {
		if self.no_default_features {
			&["--no-default-features"]
		} else {
			&[]
		}
	}

	fn features_args(&self) -> Vec<&str> {
		if self.features.is_empty() {
			vec![]
		} else {
			let mut features = vec!["--features"];
			features.extend(self.features.iter().map(String::as_str));
			features
		}
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

	fn export_syms(&self) -> Result<()> {
		let sh = sh()?;

		let input = self.build_archive();
		let output = self.dist_archive();
		sh.create_dir(output.parent().unwrap())?;
		sh.copy_file(&input, &output)?;

		let objcopy = binutil("objcopy")?;

		cmd!(sh, "{objcopy} --prefix-symbols=hermit_ {output}").run()?;

		let archive_bytes = sh.read_binary_file(&input)?;
		let archive = Archive::parse(&archive_bytes)?;

		let sys_fns = archive
			.summarize()
			.into_iter()
			.filter(|(member_name, _, _)| member_name.starts_with("hermit"))
			.flat_map(|(_, _, symbols)| symbols)
			.filter(|symbol| symbol.starts_with("sys_"));

		let explicit_exports = [
			"_start",
			"__bss_start",
			"__rg_oom",
			"runtime_entry",
			// lwIP functions (C runtime)
			"init_lwip",
			"lwip_read",
			"lwip_write",
		]
		.into_iter();

		let symbol_redefinitions = explicit_exports
			.chain(sys_fns)
			.map(|symbol| format!("hermit_{symbol} {symbol}\n"))
			.collect::<String>();

		let exported_syms = self.exported_syms();

		sh.write_file(&exported_syms, &symbol_redefinitions)?;

		cmd!(sh, "{objcopy} --redefine-syms={exported_syms} {output}").run()?;

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

	fn build_archive(&self) -> PathBuf {
		let mut built_archive = self.out_dir();
		built_archive.push("libhermit.a");
		built_archive
	}

	fn dist_archive(&self) -> PathBuf {
		let mut dist_archive = self.dist_dir();
		dist_archive.push("libhermit.a");
		dist_archive
	}

	fn exported_syms(&self) -> PathBuf {
		let mut redefine_syms_path = self.dist_dir();
		redefine_syms_path.push("exported-syms");
		redefine_syms_path
	}
}

impl flags::Clippy {
	fn run(self) -> Result<()> {
		let sh = sh()?;

		// TODO: Enable clippy for aarch64
		// https://github.com/hermitcore/libhermit-rs/issues/381
		#[allow(clippy::single_element_loop)]
		for target in ["x86_64"] {
			let target_arg = target_args(target)?;
			cmd!(sh, "cargo clippy {target_arg...}").run()?;
			cmd!(sh, "cargo clippy {target_arg...}")
				.arg("--no-default-features")
				.run()?;
			cmd!(sh, "cargo clippy {target_arg...}")
				.arg("--all-features")
				.run()?;
		}

		cmd!(sh, "cargo clippy --package xtask").run()?;

		Ok(())
	}
}

fn target(arch: &str) -> Result<&'static str> {
	match arch {
		"x86_64" => Ok("x86_64-unknown-none"),
		"aarch64" => Ok("aarch64-unknown-none-hermitkernel"),
		arch => Err(anyhow!("Unsupported arch: {arch}")),
	}
}

fn target_args(arch: &str) -> Result<&'static [&'static str]> {
	match arch {
		"x86_64" => Ok(&["--target=x86_64-unknown-none"]),
		"aarch64" => Ok(&[
			"--target=targets/aarch64-unknown-none-hermitkernel.json",
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
