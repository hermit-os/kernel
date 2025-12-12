use anyhow::Result;
use clap::Args;

use crate::arch::Arch;

/// Run rustdoc for all targets.
#[derive(Args)]
pub struct Doc;

impl Doc {
	pub fn run(self) -> Result<()> {
		let cargo_doc = |target_triple: Option<&str>, pkg_spec: &str| {
			let mut cmd = crate::cargo();
			cmd.arg("doc");
			if let Some(triple) = target_triple {
				cmd.arg(format!("--target={triple}"));
			}
			cmd.args(["--no-deps", "--document-private-items", pkg_spec]);
			eprintln!("$ {cmd:?}");
			cmd.spawn()?.wait()
		};

		for arch in Arch::all() {
			arch.install()?;
			let triple = arch.triple();
			cargo_doc(Some(triple), "--package=hermit-kernel")?;
			cargo_doc(Some(triple), "--manifest-path=hermit-builtins/Cargo.toml")?;
		}

		cargo_doc(None, "--package=xtask")?;
		Ok(())
	}
}
