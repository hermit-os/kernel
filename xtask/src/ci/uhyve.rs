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

		let uhyve = env::var("UHYVE").unwrap_or_else(|_| "uhyve".to_string());
		let program = if self.sudo { "sudo" } else { uhyve.as_str() };
		let arg = self.sudo.then_some(uhyve.as_str());
		let smp_arg = format!("--cpu-count={smp}");

		for i in 0..3 {
			eprintln!("Uhyve attempt number {}", i + 1);

			let res = cmd!(sh, "{program} {arg...} {smp_arg} {image}")
				.env("RUST_LOG", "debug")
				.run();

			match res {
				Ok(()) => break,
				Err(err) => {
					eprintln!("{err}");
					if i == 2 {
						return Err(err.into());
					}
				}
			}
		}

		Ok(())
	}
}
