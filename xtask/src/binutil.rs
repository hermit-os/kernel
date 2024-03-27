use std::path::PathBuf;

use anyhow::{anyhow, Result};

pub fn binutil(name: &str) -> Result<PathBuf> {
	let exe_suffix = std::env::consts::EXE_SUFFIX;
	let exe = format!("llvm-{name}{exe_suffix}");

	let path = llvm_tools::LlvmTools::new()
		.map_err(|err| match err {
			llvm_tools::Error::NotFound => anyhow!(
				"Could not find llvm-tools component\n\
				\n\
				Maybe the rustup component `llvm-tools` is missing? Install it through: `rustup component add llvm-tools`"
			),
			err => anyhow!("{err:?}"),
		})?
		.tool(&exe)
		.ok_or_else(|| anyhow!("could not find {exe}"))?;

	Ok(path)
}
