use std::path::{Path, PathBuf};

use clap::Args;

use crate::object::Object;
use crate::target::Target;

#[derive(Args)]
pub struct Artifact {
	/// Target.
	#[arg(value_enum, long)]
	pub target: Target,

	/// Directory for all generated artifacts.
	#[arg(long, id = "DIRECTORY")]
	pub target_dir: Option<PathBuf>,

	/// Build artifacts in release mode, with optimizations.
	#[arg(short, long)]
	pub release: bool,

	/// Build artifacts with the specified profile.
	#[arg(long, id = "PROFILE-NAME")]
	pub profile: Option<String>,
}

impl Artifact {
	pub fn profile(&self) -> &str {
		self.profile
			.as_deref()
			.unwrap_or(if self.release { "release" } else { "dev" })
	}

	pub fn profile_path_component(&self) -> &str {
		match self.profile() {
			"dev" => "debug",
			profile => profile,
		}
	}

	pub fn target_dir(&self) -> &Path {
		self.target_dir
			.as_deref()
			.unwrap_or_else(|| Path::new("target"))
	}

	pub fn build_object(&self) -> Object {
		[
			self.target_dir(),
			self.target.triple().as_ref(),
			self.profile_path_component().as_ref(),
			self.target.image_name().as_ref(),
		]
		.iter()
		.collect::<PathBuf>()
		.into()
	}

	pub fn dist_object(&self) -> Object {
		[
			self.target_dir(),
			self.profile_path_component().as_ref(),
			self.target.dist_name().as_ref(),
		]
		.iter()
		.collect::<PathBuf>()
		.into()
	}

	pub fn ci_image(&self, image: &str) -> PathBuf {
		["data", self.target.arch(), image].iter().collect()
	}
}
