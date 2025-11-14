use std::path::PathBuf;

use anyhow::Result;
use clap::{Args, Subcommand};

use crate::cargo_build::CargoBuild;

/// Work with hermit-rs images
#[derive(Args)]
pub struct Rs {
	#[command(flatten)]
	pub cargo_build: CargoBuild,

	/// Package to build (see `cargo help pkgid`)
	#[arg(short, long, id = "SPEC")]
	pub package: String,

	/// Create multiple vCPUs.
	#[arg(long, default_value_t = 1)]
	pub smp: usize,

	#[command(subcommand)]
	action: Action,
}

#[derive(Subcommand)]
pub enum Action {
	/// Build image.
	Build,
	Firecracker(super::firecracker::Firecracker),
	Qemu(super::qemu::Qemu),
	Uhyve(super::uhyve::Uhyve),
}

impl Rs {
	pub fn run(mut self) -> Result<()> {
		let image = self.build()?;

		let arch = self.cargo_build.artifact.arch;
		let small = self.cargo_build.artifact.profile() == "release";
		match self.action {
			Action::Build => Ok(()),
			Action::Firecracker(firecracker) => firecracker.run(&image, self.smp),
			Action::Qemu(qemu) => qemu.run(&image, self.smp, arch, small),
			Action::Uhyve(uhyve) => uhyve.run(&image, self.smp),
		}
	}

	pub fn build(&mut self) -> Result<PathBuf> {
		if super::in_ci() {
			eprintln!("::group::cargo build");
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

		let cargo_config = match std::env::var_os("HERMIT_KERNEL_CARGO_CONFIG") {
			Some(val) if !val.is_empty() => &[std::ffi::OsString::from("--config"), val][..],
			_ => &[],
		};

		cargo
			.current_dir(super::parent_root())
			.args(cargo_config)
			.arg("build")
			.args(self.cargo_build.artifact.arch.ci_cargo_args())
			.args(self.cargo_build.cargo_build_args())
			.args(["--package", self.package.as_str()]);

		eprintln!("$ {cargo:?}");
		let status = cargo.status()?;
		assert!(status.success());

		if super::in_ci() {
			eprintln!("::endgroup::");
		}

		Ok(self.cargo_build.artifact.ci_image(&self.package))
	}
}
