use std::path::Path;

use anyhow::Result;
use clap::Args;
use xshell::cmd;

use super::build::Build;

/// Run hermit-rs images on Firecracker.
#[derive(Args)]
pub struct Firecracker {
	#[command(flatten)]
	build: Build,
}

impl Firecracker {
	pub fn run(self) -> Result<()> {
		self.build.run()?;

		let sh = crate::sh()?;

		let config = format!(
			include_str!("firecracker_vm_config.json"),
			kernel_image_path = "hermit-loader-x86_64-fc",
			initrd_path = self.build.image().display()
		);
		eprintln!("firecracker config");
		eprintln!("{config}");
		let config_path = Path::new("firecracker_vm_config.json");
		sh.write_file(config_path, config)?;

		let log_path = Path::new("firecracker.log");
		sh.write_file(log_path, "")?;
		cmd!(sh, "firecracker --no-api --config-file {config_path} --log-path {log_path} --level Info --show-level --show-log-origin").run()?;
		let log = sh.read_file(log_path)?;

		eprintln!("firecracker log");
		eprintln!("{log}");

		Ok(())
	}
}
