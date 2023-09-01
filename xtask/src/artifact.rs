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

	fn out_dir(&self, triple: impl AsRef<Path>) -> PathBuf {
		let mut out_dir = self.target_dir().to_path_buf();
		out_dir.push(triple);
		out_dir.push(self.profile_path_component());
		out_dir
	}

	pub fn builtins_archive(&self) -> Archive {
		let mut builtins_archive = self.target_dir().to_path_buf();
		builtins_archive.push(self.arch.hermit_triple());
		builtins_archive.push("release");
		builtins_archive.push("libhermit_builtins.a");
		builtins_archive.into()
	}

	pub fn build_archive(&self) -> Archive {
		let mut built_archive = self.out_dir(self.arch.triple());
		built_archive.push("libhermit.a");
		built_archive.into()
	}

	pub fn dist_archive(&self) -> Archive {
		let mut dist_archive = self.out_dir(self.arch.name());
		dist_archive.push("libhermit.a");
		dist_archive.into()
	}
}
