use std::env;

use anyhow::Result;
use clap::Args;
use xshell::cmd;

use super::build::Build;

/// Run hermit-rs images on Uhyve.
#[derive(Args)]
pub struct Uhyve {
	/// Run Uhyve using `sudo`.
	#[arg(long)]
	sudo: bool,

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

		let uhyve = env::var("UHYVE").unwrap_or_else(|_| "uhyve".to_string());
		let program = if self.sudo { "sudo" } else { uhyve.as_str() };
		let arg = self.sudo.then_some(uhyve.as_str());

		cmd!(sh, "{program} {arg...} --verbose {image}")
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
