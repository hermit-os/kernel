//! See <https://github.com/matklad/cargo-xtask/>.

mod arch;
mod archive;
mod flags;

use std::env::{self, VarError};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use anyhow::Result;
use xshell::{cmd, Shell};

use self::arch::Arch;
use self::archive::Archive;

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
		let sh = sh()?;

		eprintln!("Building kernel");
		cmd!(sh, "cargo build")
			.env("CARGO_ENCODED_RUSTFLAGS", self.cargo_encoded_rustflags()?)
			.args(self.arch.cargo_args())
			.args(self.target_dir_args())
			.args(self.no_default_features_args())
			.args(self.features_args())
			.args(self.profile_args())
			.run()?;

		let build_archive = self.build_archive();
		let dist_archive = self.dist_archive();
		eprintln!(
			"Copying {} to {}",
			build_archive.as_ref().display(),
			dist_archive.as_ref().display()
		);
		sh.create_dir(dist_archive.as_ref().parent().unwrap())?;
		sh.copy_file(&build_archive, &dist_archive)?;

		eprintln!("Exporting symbols");
		self.export_syms()?;

		eprintln!("Building hermit-builtins");
		cmd!(sh, "cargo build")
			.arg("--manifest-path=hermit-builtins/Cargo.toml")
			.args(self.arch.builtins_cargo_args())
			.args(self.target_dir_args())
			.args(self.profile_args())
			.run()?;

		eprintln!("Exporting hermit-builtins symbols");
		let builtins = self.builtins_archive();
		let builtin_symbols = sh.read_file("hermit-builtins/exports")?;
		builtins.retain_symbols(builtin_symbols.lines())?;

		dist_archive.append(&builtins)?;

		eprintln!("Setting OSABI");
		dist_archive.set_osabi()?;

		eprintln!("Kernel available at {}", dist_archive.as_ref().display());
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

		// TODO: Re-enable mutable-noalias
		// https://github.com/hermitcore/kernel/issues/200
		rustflags.push("-Zmutable-noalias=no");

		if self.instrument_mcount {
			rustflags.push("-Zinstrument-mcount");
		}

		if self.randomize_layout {
			rustflags.push("-Zrandomize-layout")
		}

		rustflags.extend(self.arch.rustflags());

		Ok(rustflags.join("\x1f"))
	}

	fn target_dir_args(&self) -> [&OsStr; 2] {
		["--target-dir".as_ref(), self.target_dir().as_ref()]
	}

	fn no_default_features_args(&self) -> &[&str] {
		if self.no_default_features {
			&["--no-default-features"]
		} else {
			&[]
		}
	}

	fn features_args(&self) -> impl Iterator<Item = &str> {
		self.features
			.iter()
			.flat_map(|feature| ["--features", feature.as_str()])
	}

	fn profile_args(&self) -> [&str; 2] {
		["--profile", self.profile()]
	}

	fn export_syms(&self) -> Result<()> {
		let archive = self.dist_archive();

		let syscall_symbols = archive.syscall_symbols()?;
		let explicit_exports = [
			"_start",
			"__bss_start",
			"runtime_entry",
			// lwIP functions (C runtime)
			"init_lwip",
			"lwip_read",
			"lwip_write",
			// lwIP rtl8139 driver
			"init_rtl8139_netif",
			"irq_install_handler",
			"virt_to_phys",
			"eoi",
		]
		.into_iter();

		let symbols = explicit_exports.chain(syscall_symbols.iter().map(String::as_str));

		archive.retain_symbols(symbols)?;

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

	fn out_dir(&self, triple: impl AsRef<Path>) -> PathBuf {
		let mut out_dir = self.target_dir().to_path_buf();
		out_dir.push(triple);
		out_dir.push(match self.profile() {
			"dev" => "debug",
			profile => profile,
		});
		out_dir
	}

	fn builtins_archive(&self) -> Archive {
		let mut builtins_archive = self.out_dir(self.arch.hermit_triple());
		builtins_archive.push("libhermit_builtins.a");
		builtins_archive.into()
	}

	fn build_archive(&self) -> Archive {
		let mut built_archive = self.out_dir(self.arch.triple());
		built_archive.push("libhermit.a");
		built_archive.into()
	}

	fn dist_archive(&self) -> Archive {
		let mut dist_archive = self.out_dir(self.arch.name());
		dist_archive.push("libhermit.a");
		dist_archive.into()
	}
}

impl flags::Clippy {
	fn run(self) -> Result<()> {
		let sh = sh()?;

		for target in [Arch::X86_64, Arch::AArch64] {
			let target_args = target.cargo_args();
			cmd!(sh, "cargo clippy {target_args...}").run()?;
			cmd!(sh, "cargo clippy {target_args...}")
				.arg("--no-default-features")
				.run()?;
			cmd!(sh, "cargo clippy {target_args...}")
				.arg("--no-default-features")
				.arg("--features=acpi,fsgsbase,pci,smp,vga")
				.run()?;
			// TODO: Enable clippy for newlib
			// https://github.com/hermitcore/kernel/issues/470
			// cmd!(sh, "cargo clippy {target_args...}")
			// 	.arg("--no-default-features")
			// 	.arg("--features=acpi,fsgsbase,newlib,smp,vga")
			// 	.run()?;
		}

		cmd!(sh, "cargo clippy")
			.arg("--manifest-path=hermit-builtins/Cargo.toml")
			.arg("--target=x86_64-unknown-none")
			.run()?;

		cmd!(sh, "cargo clippy --package xtask").run()?;

		Ok(())
	}
}

fn sh() -> Result<Shell> {
	let sh = Shell::new()?;
	let project_root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
	sh.change_dir(project_root);
	Ok(sh)
}
