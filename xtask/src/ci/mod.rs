use anyhow::Result;
use clap::Subcommand;

mod build;
mod firecracker;
mod qemu;
mod uhyve;

/// Run CI tasks.
#[derive(Subcommand)]
pub enum Ci {
	Build(build::Build),
	Firecracker(firecracker::Firecracker),
	Qemu(qemu::Qemu),
	Uhyve(uhyve::Uhyve),
}

impl Ci {
	pub fn run(self) -> Result<()> {
		match self {
			Self::Build(build) => build.run(),
			Self::Firecracker(firecracker) => firecracker.run(),
			Self::Qemu(qemu) => qemu.run(),
			Self::Uhyve(uhyve) => uhyve.run(),
		}
	}
}

fn in_ci() -> bool {
	std::env::var_os("CI") == Some("true".into())
}
