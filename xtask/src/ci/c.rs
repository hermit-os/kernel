use std::path::PathBuf;

use anyhow::Result;
use clap::{Args, Subcommand};
use xshell::cmd;

use crate::arch::Arch;

/// Work with hermit-c images
#[derive(Args)]
pub struct C {
	/// Target architecture.
	#[arg(value_enum, long)]
	pub arch: Arch,

	/// Build type to use.
	#[arg(long, default_value = "debug")]
	pub buildtype: String,

	/// Target to build.
	#[arg(long, id = "SPEC")]
	pub target: String,

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

impl C {
	pub fn run(mut self) -> Result<()> {
		let image = self.build()?;

		match self.action {
			Action::Build => Ok(()),
			Action::Firecracker(firecracker) => firecracker.run(&image, self.smp),
			Action::Qemu(qemu) => qemu.run(&image, self.smp, self.arch, false),
			Action::Uhyve(uhyve) => uhyve.run(&image, self.smp),
		}
	}

	pub fn build(&mut self) -> Result<PathBuf> {
		if super::in_ci() {
			eprintln!("::group::meson compile");
		}

		let arch = self.arch.name();
		let buildtype = self.buildtype.as_str();
		let target = self.target.as_str();
		let build_dir = format!("build-{arch}-hermit-{buildtype}");

		let sh = crate::sh()?;
		sh.change_dir(super::parent_root());

		cmd!(
			sh,
			"meson setup --buildtype {buildtype} --cross-file cross/{arch}-hermit.ini {build_dir}"
		)
		.run()?;

		cmd!(sh, "meson setup --reconfigure {build_dir}").run()?;

		cmd!(sh, "meson compile -C {build_dir} -v {target}").run()?;

		let image = {
			let mut image = super::parent_root().to_path_buf();
			image.push(build_dir);
			image.push(target);
			image
		};

		if super::in_ci() {
			eprintln!("::endgroup::");
		}

		Ok(image)
	}
}
