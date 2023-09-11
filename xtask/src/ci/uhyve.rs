use anyhow::Result;
use clap::Args;
use xshell::cmd;

use super::build::Build;

/// Run hermit-rs images on Uhyve.
#[derive(Args)]
pub struct Uhyve {
	/// Run with multiple vCPUs.
	#[arg(long)]
	smp: bool,

	#[command(flatten)]
	build: Build,
}

impl Uhyve {
	pub fn run(self) -> Result<()> {
		self.build.run()?;

		let sh = crate::sh()?;

		let image = self.build.image();

		cmd!(sh, "uhyve --verbose {image}")
			.env("RUST_LOG", "debug")
			.args(self.cpu_count_args())
			.run()?;

		Ok(())
	}

	fn cpu_count_args(&self) -> &'static [&'static str] {
		if self.smp {
			&["--cpu-count=4"]
		} else {
			&[]
		}
	}
}
