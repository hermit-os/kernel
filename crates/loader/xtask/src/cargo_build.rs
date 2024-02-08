use std::ffi::OsStr;

use clap::Args;
use xshell::Cmd;

use crate::artifact::Artifact;

#[derive(Args)]
pub struct CargoBuild {
	#[command(flatten)]
	pub artifact: Artifact,

	/// Do not activate the `default` feature.
	#[arg(long)]
	no_default_features: bool,

	/// Space or comma separated list of features to activate.
	#[arg(long)]
	pub features: Vec<String>,
}

pub trait CmdExt {
	fn cargo_build_args(self, cargo_build: &CargoBuild) -> Self;
	fn target_dir_args(self, cargo_build: &CargoBuild) -> Self;
}

impl CmdExt for Cmd<'_> {
	fn cargo_build_args(self, cargo_build: &CargoBuild) -> Self {
		let cmd = self
			.target_dir_args(cargo_build)
			.args(cargo_build.no_default_features_args())
			.args(cargo_build.features_args())
			.args(cargo_build.release_args());

		if let Some(profile) = &cargo_build.artifact.profile {
			cmd.args(&["--profile", profile])
		} else {
			cmd
		}
	}

	fn target_dir_args(self, cargo_build: &CargoBuild) -> Self {
		if let Some(target_dir) = &cargo_build.artifact.target_dir {
			self.args::<&[&OsStr]>(&["--target-dir".as_ref(), target_dir.as_ref()])
		} else {
			self
		}
	}
}

impl CargoBuild {
	fn release_args(&self) -> &'static [&'static str] {
		if self.artifact.release {
			&["--release"]
		} else {
			&[]
		}
	}

	fn no_default_features_args(&self) -> &'static [&'static str] {
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
}
