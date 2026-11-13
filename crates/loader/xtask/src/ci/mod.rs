use anyhow::Result;
use clap::Subcommand;

mod firecracker;
mod qemu;

/// Run CI tasks.
#[derive(Subcommand)]
pub enum Ci {
	Firecracker(firecracker::Firecracker),
	Qemu(qemu::Qemu),
}

impl Ci {
	pub fn run(self) -> Result<()> {
		match self {
			Self::Firecracker(firecracker) => firecracker.run(),
			Self::Qemu(qemu) => qemu.run(),
		}
	}
}

fn in_ci() -> bool {
	std::env::var_os("CI") == Some("true".into())
}
