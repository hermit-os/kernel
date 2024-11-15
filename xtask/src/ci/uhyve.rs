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

	#[command(flatten)]
	build: Build,
}

impl Uhyve {
	pub fn run(mut self) -> Result<()> {
		self.build.run()?;

		let sh = crate::sh()?;

		let image = self.build.image();

		let uhyve = env::var("UHYVE").unwrap_or_else(|_| "uhyve".to_string());
		let program = if self.sudo { "sudo" } else { uhyve.as_str() };
		let arg = self.sudo.then_some(uhyve.as_str());

		cmd!(sh, "{program} {arg...} {image}")
			.env("RUST_LOG", "debug")
			.arg(self.cpu_count_arg())
			.run()?;

		Ok(())
	}

	fn cpu_count_arg(&self) -> String {
		let smp = self.build.smp;
		format!("--cpu-count={}", smp)
	}
}
