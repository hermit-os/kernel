use std::env::{self, VarError};

use anyhow::Result;
use clap::Args;

use crate::cargo_build::CargoBuild;

/// Build the kernel.
#[derive(Args)]
pub struct Build {
	#[command(flatten)]
	cargo_build: CargoBuild,

	/// Deprecated: use `--features instrument-mcount` instead.
	#[arg(long)]
	pub instrument_mcount: bool,

	/// Deprecated: use `--features randomize-layout` instead.
	#[arg(long)]
	pub randomize_layout: bool,
}

impl Build {
	pub fn run(mut self) -> Result<()> {
		let sh = crate::sh()?;

		if self.instrument_mcount {
			self.cargo_build
				.features
				.push("instrument-mcount".to_owned());
		}

		if self.randomize_layout {
			self.cargo_build
				.features
				.push("randomize-layout".to_owned());
		}

		self.cargo_build.artifact.arch.install_for_build()?;
		let dist_archive = self.cargo_build.artifact.dist_archive();
		sh.create_dir(dist_archive.as_ref().parent().unwrap())?;
		sh.remove_path(&dist_archive)?;
		dist_archive.create()?;

		if self
			.cargo_build
			.features()
			.any(|feature| feature == "masos")
		{
			self.build_builtins(true)?;
		} else {
			self.build_kernel()?;

			self.build_builtins(false)?;
		}

		eprintln!("Setting OSABI");
		dist_archive.set_osabi()?;

		eprintln!("Kernel available at {}", dist_archive.as_ref().display());
		Ok(())
	}

	fn build_kernel(&self) -> Result<()> {
		let careful = match env::var_os("HERMIT_CAREFUL") {
			Some(val) if val == "1" => &["careful"][..],
			_ => &[],
		};

		eprintln!("Building kernel");
		let mut cargo = crate::cargo();
		cargo
			.args(careful)
			.arg("rustc")
			.arg("--crate-type=staticlib")
			.env("CARGO_ENCODED_RUSTFLAGS", self.cargo_encoded_rustflags()?)
			.args(self.cargo_build.artifact.arch.cargo_args())
			.args(self.cargo_build.cargo_build_args());

		eprintln!("$ {cargo:?}");
		let status = cargo.status()?;
		assert!(status.success());

		let build_archive = self.cargo_build.artifact.build_archive();
		let dist_archive = self.cargo_build.artifact.dist_archive();

		build_archive.retain_kernel_symbols()?;
		dist_archive.append(&build_archive)?;

		Ok(())
	}

	fn build_builtins(&self, masos: bool) -> Result<()> {
		eprintln!("Building hermit-builtins");
		let mut cargo = crate::cargo();
		cargo
			.arg("build")
			.arg("--manifest-path=hermit-builtins/Cargo.toml")
			.arg("--profile")
			.arg(self.cargo_build.artifact.builtins_profile_path_component())
			.args(self.cargo_build.artifact.arch.builtins_cargo_args())
			.args(self.cargo_build.builtins_target_dir_arg());
		if masos {
			cargo.arg("--features=masos");
		}

		eprintln!("$ {cargo:?}");
		let status = cargo.status()?;
		assert!(status.success());

		let builtins_archive = self.cargo_build.artifact.builtins_archive();
		let dist_archive = self.cargo_build.artifact.dist_archive();

		if masos {
			builtins_archive.retain_masos_symbols()?;
		} else {
			builtins_archive.retain_builtin_symbols()?;
		}
		dist_archive.append(&builtins_archive)?;

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

		if self
			.cargo_build
			.features()
			.any(|feature| feature == "instrument-mcount")
		{
			rustflags.push("-Zinstrument-mcount");
			rustflags.push("-Cpasses=ee-instrument<post-inline>");
		}

		if self
			.cargo_build
			.features()
			.any(|feature| feature == "randomize-layout")
		{
			rustflags.push("-Zrandomize-layout");
		}

		rustflags.extend(self.cargo_build.artifact.arch.rustflags());

		Ok(rustflags.join("\x1f"))
	}
}
