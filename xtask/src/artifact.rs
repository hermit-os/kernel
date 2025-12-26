use std::path::{self, PathBuf};

use clap::Args;

use crate::arch::Arch;
use crate::archive::Archive;

#[derive(Args)]
pub struct Artifact {
	/// Target architecture.
	#[arg(value_enum, long)]
	pub arch: Arch,

	/// Directory for all generated artifacts.
	#[arg(long, id = "DIRECTORY")]
	pub target_dir: Option<PathBuf>,

	/// Copy final artifacts to this directory
	#[arg(long, id = "PATH")]
	pub artifact_dir: Option<PathBuf>,

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

	pub fn target_dir(&self) -> PathBuf {
		if let Some(target_dir) = &self.target_dir {
			return path::absolute(target_dir).unwrap();
		}

		crate::project_root().join("target")
	}

	pub fn builtins_target_dir(&self) -> PathBuf {
		self.target_dir().join("hermit-builtins")
	}

	pub fn builtins_archive(&self) -> Archive {
		[
			self.builtins_target_dir().as_path(),
			self.arch.hermit_triple().as_ref(),
			"release".as_ref(),
			"libhermit_builtins.a".as_ref(),
		]
		.iter()
		.collect::<PathBuf>()
		.into()
	}

	pub fn build_archive(&self) -> Archive {
		[
			self.target_dir().as_path(),
			self.arch.triple().as_ref(),
			self.profile_path_component().as_ref(),
			"libhermit.a".as_ref(),
		]
		.iter()
		.collect::<PathBuf>()
		.into()
	}

	fn artifact_dir(&self) -> PathBuf {
		if let Some(artifact_dir) = &self.artifact_dir {
			return path::absolute(artifact_dir).unwrap();
		}

		[
			self.target_dir().as_path(),
			self.arch.name().as_ref(),
			self.profile_path_component().as_ref(),
		]
		.iter()
		.collect()
	}

	pub fn dist_archive(&self) -> Archive {
		self.artifact_dir().join("libhermit.a").into()
	}

	#[cfg(feature = "ci")]
	pub fn ci_image(&self, package: &str) -> PathBuf {
		[
			crate::ci::parent_root(),
			"target".as_ref(),
			self.arch.hermit_triple().as_ref(),
			self.profile_path_component().as_ref(),
			package.as_ref(),
		]
		.iter()
		.collect()
	}
}
