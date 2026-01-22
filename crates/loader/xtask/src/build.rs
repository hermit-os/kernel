use std::env::{self, VarError};
use std::path::PathBuf;

use anyhow::Result;
use clap::Args;
use xshell::cmd;

use crate::artifact::{Artifact, CmdExt};
use crate::target::Target;

/// Build the kernel.
#[derive(Args)]
pub struct Build {
	#[command(flatten)]
	pub artifact: Artifact,
}

impl Build {
	pub fn run(&self) -> Result<()> {
		self.artifact.target.install()?;

		let sh = crate::sh()?;

		eprintln!("Building loader");
		cmd!(sh, "cargo build")
			.env("CARGO_ENCODED_RUSTFLAGS", self.cargo_encoded_rustflags()?)
			.args(self.artifact.target.cargo_args())
			.args(self.artifact.target.feature_flags())
			.cargo_build_args(&self.artifact)
			.run()?;

		let build_object = self.artifact.build_object();
		let dist_object = self.artifact.dist_object();
		eprintln!(
			"Copying {} to {}",
			build_object.as_ref().display(),
			dist_object.as_ref().display()
		);
		sh.create_dir(dist_object.as_ref().parent().unwrap())?;
		sh.copy_file(&build_object, &dist_object)?;

		if self.artifact.target == Target::X86_64Multiboot {
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

		rustflags.extend(self.artifact.target.rustflags());

		Ok(rustflags.join("\x1f"))
	}

	pub fn dist_object(&self) -> PathBuf {
		self.artifact.dist_object().into()
	}

	pub fn target(&self) -> Target {
		self.artifact.target
	}

	pub fn ci_image(&self, image: &str) -> PathBuf {
		self.artifact.ci_image(image)
	}
}
