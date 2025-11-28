use std::ffi::OsString;

use clap::Args;

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

impl CargoBuild {
	pub fn cargo_build_args(&self) -> Vec<String> {
		let mut args = vec![];
		args.extend(self.target_dir_args());
		args.extend(
			self.no_default_features_args()
				.iter()
				.map(|&s| s.to_owned()),
		);
		args.extend(self.features_args().map(|s| s.to_owned()));
		args.extend(self.release_args().iter().map(|&s| s.to_owned()));
		if let Some(profile) = &self.artifact.profile {
			args.push("--profile".to_owned());
			args.push(profile.to_owned());
		}
		args
	}

	pub fn target_dir_args(&self) -> Vec<String> {
		if self.artifact.target_dir.is_some() {
			vec![
				"--target-dir".to_owned(),
				self.artifact
					.target_dir()
					.into_os_string()
					.into_string()
					.unwrap(),
			]
		} else {
			vec![]
		}
	}

	pub fn builtins_target_dir_arg(&self) -> [OsString; 2] {
		[
			OsString::from("--target-dir"),
			self.artifact.builtins_target_dir().into_os_string(),
		]
	}

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
