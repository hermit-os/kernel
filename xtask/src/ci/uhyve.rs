use std::env;
use std::path::Path;

use anyhow::Result;
use clap::Args;
use xshell::cmd;

/// Run image on Uhyve.
#[derive(Args)]
pub struct Uhyve {
	/// Run Uhyve using `sudo`.
	#[arg(long)]
	sudo: bool,
}

impl Uhyve {
	pub fn run(self, image: &Path, smp: usize) -> Result<()> {
		let sh = crate::sh()?;

		let uhyve = env::var("UHYVE").unwrap_or_else(|_| "uhyve".to_owned());
		let program = if self.sudo { "sudo" } else { uhyve.as_str() };
		let arg = self.sudo.then_some(uhyve.as_str());
		let smp_arg = format!("--cpu-count={smp}");

		cmd!(sh, "{program} {arg...} {smp_arg} {image}")
			.env("RUST_LOG", "debug")
			.run()?;

		Ok(())
	}
}
