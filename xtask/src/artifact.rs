use std::path::{Path, PathBuf};

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

	pub fn builtins_archive(&self) -> Archive {
		[
			"hermit-builtins".as_ref(),
			self.target_dir(),
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
			self.target_dir(),
			self.arch.triple().as_ref(),
			self.profile_path_component().as_ref(),
			"libhermit.a".as_ref(),
		]
		.iter()
		.collect::<PathBuf>()
		.into()
	}

	pub fn dist_archive(&self) -> Archive {
		[
			self.target_dir(),
			self.arch.name().as_ref(),
			self.profile_path_component().as_ref(),
			"libhermit.a".as_ref(),
		]
		.iter()
		.collect::<PathBuf>()
		.into()
	}

	pub fn ci_image(&self, package: &str) -> PathBuf {
		[
			"..".as_ref(),
			self.target_dir(),
			self.arch.hermit_triple().as_ref(),
			self.profile_path_component().as_ref(),
			package.as_ref(),
		]
		.iter()
		.collect()
	}
}
