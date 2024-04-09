use std::env;
use std::path::PathBuf;
use std::process::Command;

use anyhow::Result;
use clap::Args;

use crate::cargo_build::CargoBuild;

/// Build hermit-rs images.
#[derive(Args)]
#[command(next_help_heading = "Build options")]
pub struct Build {
	#[command(flatten)]
	pub cargo_build: CargoBuild,

	/// Package to build (see `cargo help pkgid`)
	#[arg(short, long, id = "SPEC")]
	pub package: String,
}

impl Build {
	pub fn run(&self) -> Result<()> {
		if super::in_ci() {
			eprintln!("::group::cargo build")
		}

		let mut cargo = cargo();

		if self.package.contains("rftrace") {
			cargo.env(
				"RUSTFLAGS",
				"-Zinstrument-mcount -Cpasses=ee-instrument<post-inline>",
			);
		};

		cargo
			.current_dir("..")
			.arg("build")
			.args(self.cargo_build.artifact.arch.ci_cargo_args())
			.args(self.cargo_build.cargo_build_args())
			.args(["--package", self.package.as_str()]);

		eprintln!("$ {cargo:?}");
		let status = cargo.status()?;
		assert!(status.success());

		if super::in_ci() {
			eprintln!("::endgroup::")
		}

		Ok(())
	}

	pub fn image(&self) -> PathBuf {
		self.cargo_build.artifact.ci_image(&self.package)
	}
}

fn cargo() -> Command {
	let cargo = {
		let exe = format!("cargo{}", env::consts::EXE_SUFFIX);
		// On windows, the userspace toolchain ends up in front of the rustup proxy in $PATH.
		// To reach the rustup proxy nonetheless, we explicitly query $CARGO_HOME.
		let mut cargo_home = PathBuf::from(env::var_os("CARGO_HOME").unwrap());
		cargo_home.push("bin");
		cargo_home.push(&exe);
		if cargo_home.exists() {
			cargo_home
		} else {
			PathBuf::from(exe)
		}
	};

	let mut cargo = Command::new(cargo);

	// Remove rust-toolchain-specific environment variables from kernel cargo
	cargo.env_remove("LD_LIBRARY_PATH");
	env::vars()
		.filter(|(key, _value)| key.starts_with("CARGO") || key.starts_with("RUST"))
		.for_each(|(key, _value)| {
			cargo.env_remove(&key);
		});

	cargo
}
