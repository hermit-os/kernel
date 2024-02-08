use std::env::{self, VarError};
use std::path::PathBuf;

use anyhow::Result;
use clap::Args;
use xshell::cmd;

use crate::cargo_build::{CargoBuild, CmdExt};
use crate::target::Target;

/// Build the kernel.
#[derive(Args)]
pub struct Build {
	#[command(flatten)]
	cargo_build: CargoBuild,
}

impl Build {
	pub fn run(&self) -> Result<()> {
		self.cargo_build.artifact.target.install()?;

		let sh = crate::sh()?;

		eprintln!("Building loader");
		cmd!(sh, "cargo build")
			.env("CARGO_ENCODED_RUSTFLAGS", self.cargo_encoded_rustflags()?)
			.args(self.cargo_build.artifact.target.cargo_args())
			.cargo_build_args(&self.cargo_build)
			.run()?;

		let build_object = self.cargo_build.artifact.build_object();
		let dist_object = self.cargo_build.artifact.dist_object();
		eprintln!(
			"Copying {} to {}",
			build_object.as_ref().display(),
			dist_object.as_ref().display()
		);
		sh.create_dir(dist_object.as_ref().parent().unwrap())?;
		sh.copy_file(&build_object, &dist_object)?;

		if self.cargo_build.artifact.target == Target::X86_64 {
			eprintln!("Converting object to elf32-i386");
			dist_object.convert_to_elf32_i386()?;
		}

		eprintln!("Loader available at {}", dist_object.as_ref().display());
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

		rustflags.extend(self.cargo_build.artifact.target.rustflags());

		Ok(rustflags.join("\x1f"))
	}

	pub fn dist_object(&self) -> PathBuf {
		self.cargo_build.artifact.dist_object().into()
	}

	pub fn target(&self) -> Target {
		self.cargo_build.artifact.target
	}

	pub fn ci_image(&self, image: &str) -> PathBuf {
		self.cargo_build.artifact.ci_image(image)
	}
}
