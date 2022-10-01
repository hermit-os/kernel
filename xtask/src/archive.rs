use std::{
	env,
	path::{Path, PathBuf},
};

use anyhow::{anyhow, Result};
use goblin::{archive::Archive as GoblinArchive, elf64::header};
use llvm_tools::LlvmTools;
use xshell::cmd;

pub struct Archive(PathBuf);

impl From<PathBuf> for Archive {
	fn from(archive: PathBuf) -> Self {
		Self(archive)
	}
}

impl AsRef<Path> for Archive {
	fn as_ref(&self) -> &Path {
		&self.0
	}
}

impl Archive {
	pub fn syscall_symbols(&self) -> Result<Vec<String>> {
		let sh = crate::sh()?;
		let archive = self.as_ref();

		let archive_bytes = sh.read_binary_file(archive)?;
		let archive = GoblinArchive::parse(&archive_bytes)?;
		let symbols = archive
			.summarize()
			.into_iter()
			.filter(|(member_name, _, _)| member_name.starts_with("hermit-"))
			.flat_map(|(_, _, symbols)| symbols)
			.filter(|symbol| symbol.starts_with("sys_"))
			.map(String::from)
			.collect();

		Ok(symbols)
	}

	pub fn retain_symbols<'a>(&self, symbols: impl Iterator<Item = &'a str>) -> Result<()> {
		let sh = crate::sh()?;
		let archive = self.as_ref();
		let prefix = archive.file_stem().unwrap().to_str().unwrap();

		let symbol_renames = symbols
			.map(|symbol| format!("{prefix}_{symbol} {symbol}\n"))
			.collect::<String>();

		let rename_path = archive.with_extension("redefine-syms");
		sh.write_file(&rename_path, &symbol_renames)?;

		let objcopy = binutil("objcopy")?;
		cmd!(sh, "{objcopy} --prefix-symbols={prefix}_ {archive}").run()?;
		cmd!(sh, "{objcopy} --redefine-syms={rename_path} {archive}").run()?;

		sh.remove_path(&rename_path)?;

		Ok(())
	}

	pub fn append(&self, file: &Self) -> Result<()> {
		let sh = crate::sh()?;
		let archive = self.as_ref();
		let file = file.as_ref();

		let ar = binutil("ar")?;
		cmd!(sh, "{ar} qL {archive} {file}").run()?;

		Ok(())
	}

	pub fn set_osabi(&self) -> Result<()> {
		let sh = crate::sh()?;
		let archive_path = self.as_ref();

		let mut archive_bytes = sh.read_binary_file(&archive_path)?;
		let archive = GoblinArchive::parse(&archive_bytes)?;

		let file_offsets = (0..archive.len())
			.map(|i| archive.get_at(i).unwrap().offset)
			.collect::<Vec<_>>();

		for file_offset in file_offsets {
			let file_offset = usize::try_from(file_offset).unwrap();
			archive_bytes[file_offset + header::EI_OSABI] = header::ELFOSABI_STANDALONE;
		}

		sh.write_file(&archive_path, archive_bytes)?;

		Ok(())
	}
}

fn binutil(name: &str) -> Result<PathBuf> {
	let exe_suffix = env::consts::EXE_SUFFIX;
	let exe = format!("llvm-{name}{exe_suffix}");

	let path = LlvmTools::new()
		.map_err(|err| anyhow!("{err:?}"))?
		.tool(&exe)
		.ok_or_else(|| anyhow!("could not find {exe}"))?;

	Ok(path)
}
