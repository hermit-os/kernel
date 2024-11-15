use std::path::PathBuf;

use anyhow::Result;
use clap::Args;

use crate::cargo_build::CargoBuild;

/// Build hermit-rs images.
#[derive(Args)]
#[command(next_help_heading = "Build options")]
pub struct Build {
	#[command(flatten)]
	pub cargo_build: CargoBuild,

	/// Package to build (see `cargo help pkgid`)
	#[arg(short, long, id = "SPEC")]
	pub package: String,

	/// Create multiple vCPUs.
	#[arg(long, default_value_t = 1)]
	pub smp: usize,
}

impl Build {
	pub fn run(&mut self) -> Result<()> {
		if super::in_ci() {
			eprintln!("::group::cargo build")
		}

		if self.smp > 1 {
			self.cargo_build.features.push("hermit/smp".to_string());
		}

		let mut cargo = crate::cargo();

		if self.package.contains("rftrace") {
			cargo.env(
				"RUSTFLAGS",
				"-Zinstrument-mcount -Cpasses=ee-instrument<post-inline>",
			);
		};

		cargo
			.current_dir(super::parent_root())
			.arg("build")
			.args(self.cargo_build.artifact.arch.ci_cargo_args())
			.args(self.cargo_build.cargo_build_args())
			.args(["--package", self.package.as_str()]);

		eprintln!("$ {cargo:?}");
		let status = cargo.status()?;
		assert!(status.success());

		if super::in_ci() {
			eprintln!("::endgroup::")
		}

		Ok(())
	}

	pub fn image(&self) -> PathBuf {
		self.cargo_build.artifact.ci_image(&self.package)
	}
}
