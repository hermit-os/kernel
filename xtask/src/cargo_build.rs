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
				.map(|s| s.to_string()),
		);
		args.extend(self.features_args().map(|s| s.to_string()));
		args.extend(self.release_args().iter().map(|s| s.to_string()));
		if let Some(profile) = &self.artifact.profile {
			args.push("--profile".to_string());
			args.push(profile.to_string());
		}
		args
	}

	pub fn target_dir_args(&self) -> Vec<String> {
		if let Some(target_dir) = &self.artifact.target_dir {
			vec![
				"--target-dir".to_string(),
				target_dir.to_str().unwrap().to_string(),
			]
		} else {
			vec![]
		}
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
