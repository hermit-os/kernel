use std::env;
use std::path::Path;

use anyhow::Result;
use clap::Args;
use xshell::cmd;

/// Run image on Firecracker.
#[derive(Args)]
pub struct Firecracker {
	/// Run Firecracker using `sudo`.
	#[arg(long)]
	sudo: bool,
}

impl Firecracker {
	pub fn run(self, image: &Path, smp: usize) -> Result<()> {
		let sh = crate::sh()?;

		let config = format!(
			include_str!("firecracker_vm_config.json"),
			kernel_image_path = "hermit-loader-x86_64-fc",
			initrd_path = image.display(),
			vcpu_count = smp,
		);
		eprintln!("firecracker config");
		eprintln!("{config}");
		let config_path = Path::new("firecracker_vm_config.json");
		sh.write_file(config_path, config)?;

		let firecracker = env::var("FIRECRACKER").unwrap_or_else(|_| "firecracker".to_owned());
		let program = if self.sudo {
			"sudo"
		} else {
			firecracker.as_str()
		};
		let arg = self.sudo.then_some(firecracker.as_str());

		for run in 1.. {
			let log_path = Path::new("firecracker.log");
			sh.write_file(log_path, "")?;
			let res = cmd!(sh, "{program} {arg...} --no-api --config-file {config_path} --log-path {log_path} --level Info --show-level --show-log-origin").run();
			let log = sh.read_file(log_path)?;

			eprintln!("firecracker log");
			eprintln!("{log}");

			match res {
				Ok(()) => break,
				Err(err) => {
					eprintln!("::error::Firecracker attempt number {run} failed: {err}");

					if run == 5 {
						return Err(err.into());
					}
				}
			}
		}

		Ok(())
	}
}
