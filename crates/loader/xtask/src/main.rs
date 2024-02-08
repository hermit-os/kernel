//! See <https://github.com/matklad/cargo-xtask/>.

mod artifact;
mod build;
mod cargo_build;
mod ci;
mod clippy;
mod object;
mod target;

use std::env;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};
use clap::Parser;

#[derive(Parser)]
enum Cli {
	Build(build::Build),
	#[command(subcommand)]
	Ci(ci::Ci),
	Clippy(clippy::Clippy),
}

impl Cli {
	fn run(self) -> Result<()> {
		match self {
			Self::Build(build) => build.run(),
			Self::Ci(ci) => ci.run(),
			Self::Clippy(clippy) => clippy.run(),
		}
	}
}

fn main() -> Result<()> {
	let cli = Cli::parse();
	cli.run()
}

pub fn sh() -> Result<xshell::Shell> {
	let sh = xshell::Shell::new()?;
	let project_root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
	sh.change_dir(project_root);
	Ok(sh)
}

pub fn binutil(name: &str) -> Result<PathBuf> {
	let exe_suffix = env::consts::EXE_SUFFIX;
	let exe = format!("llvm-{name}{exe_suffix}");

	let path = llvm_tools::LlvmTools::new()
		.map_err(|err| anyhow!("{err:?}"))?
		.tool(&exe)
		.ok_or_else(|| anyhow!("could not find {exe}"))?;

	Ok(path)
}
