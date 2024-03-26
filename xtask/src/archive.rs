use std::collections::HashSet;
use std::fmt::Write;
use std::path::{Path, PathBuf};

use anyhow::Result;
use goblin::archive::Archive as GoblinArchive;
use goblin::elf64::header;
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

	pub fn retain_symbols(&self, mut exported_symbols: HashSet<&str>) -> Result<()> {
		let sh = crate::sh()?;
		let archive = self.as_ref();
		let prefix = {
			let file_stem = archive.file_stem().unwrap().to_str().unwrap();
			file_stem.strip_prefix("lib").unwrap_or(file_stem)
		};

		let all_symbols = {
			let nm = crate::binutil("nm")?;
			let stdout = cmd!(sh, "{nm} --export-symbols {archive}").output()?.stdout;
			String::from_utf8(stdout)?
		};

		let symbol_renames = all_symbols
			.lines()
			.fold(String::new(), |mut output, symbol| {
				if exported_symbols.remove(symbol) {
					return output;
				}

				if let Some(symbol) = symbol.strip_prefix("_ZN") {
					let prefix_len = prefix.len();
					let _ = writeln!(output, "_ZN{symbol} _ZN{prefix_len}{prefix}{symbol}",);
				} else {
					let _ = writeln!(output, "{symbol} {prefix}_{symbol}");
				}
				output
			});

		let rename_path = archive.with_extension("redefine-syms");
		sh.write_file(&rename_path, symbol_renames)?;

		let objcopy = crate::binutil("objcopy")?;
		cmd!(sh, "{objcopy} --redefine-syms={rename_path} {archive}").run()?;

		sh.remove_path(&rename_path)?;

		Ok(())
	}

	pub fn append(&self, file: &Self) -> Result<()> {
		let sh = crate::sh()?;
		let archive = self.as_ref();
		let file = file.as_ref();

		let ar = crate::binutil("ar")?;
		cmd!(sh, "{ar} qL {archive} {file}").run()?;

		Ok(())
	}

	pub fn set_osabi(&self) -> Result<()> {
		let sh = crate::sh()?;
		let archive_path = self.as_ref();

		let mut archive_bytes = sh.read_binary_file(archive_path)?;
		let archive = GoblinArchive::parse(&archive_bytes)?;

		let file_offsets = (0..archive.len())
			.map(|i| archive.get_at(i).unwrap().offset)
			.collect::<Vec<_>>();

		for file_offset in file_offsets {
			let file_offset = usize::try_from(file_offset).unwrap();
			archive_bytes[file_offset + header::EI_OSABI] = header::ELFOSABI_STANDALONE;
		}

		sh.write_file(archive_path, archive_bytes)?;

		Ok(())
	}
}
