use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

/// A helper struct to parse PE files and give them the Linux Boot Protocol attributes
/// that QEMU expects
pub(crate) struct PEFile {
	original_data: Vec<u8>,
	output_handle: File,
}

/// Position of the new PE header. 0x40 is the end of the DOS header, it cannot be earlier.
const PE_HEADER_POS: usize = 0x40;

/// The size of the Optional Section of the header
const LINUX_MAGIC_POS: usize = 0x202;
const LINUX_MAX_INITRD_SIZE_POS: usize = 0x22C;

const SYMBOL_TABLE_SIZE: usize = 40;

const MAX_OPT_HEADER_SIZE: usize = 0xa0;

// Helpful reference: https://upload.wikimedia.org/wikipedia/commons/1/1b/Portable_Executable_32_bit_Structure_in_SVG_fixed.svg
// (but it's for PE32, and we have a PE32+, so cross-reference with https://learn.microsoft.com/en-us/windows/win32/debug/pe-format#optional-header-image-only)

impl PEFile {
	/// Write the first 0x40 bytes of the PE file, corresponding to the DOS section
	fn write_dos_header(&mut self) {
		let mut dos_header = vec![0x00u8; 0x40];
		dos_header[0] = 0x4D;
		dos_header[1] = 0x5A;
		dos_header.as_mut_slice()[0x38..0x3C].copy_from_slice(
			&[0xcd, 0x23, 0x82, 0x81], // base DOS code, does nothing
		);
		dos_header.as_mut_slice()[0x3C..0x40]
			.copy_from_slice(&u32::try_from(PE_HEADER_POS).unwrap().to_le_bytes()[0..4]);

		self.output_handle
			.write_all(dos_header.as_slice())
			.expect("failed to write DOS header");
	}

	/// Reads the original PE header position
	fn pe_header_pos(&self) -> usize {
		let pe_header_pos: [u8; 4] = (&self.original_data[0x3C..0x40]).try_into().unwrap();
		let pe_header_pos = u32::from_le_bytes(pe_header_pos);

		usize::try_from(pe_header_pos).unwrap()
	}

	/// Write the PE header of this file. Comes just after the DOS header.
	fn write_pe_header(&mut self) {
		const PE_MANDATORY_HEADER_SIZE: usize = 0x18;
		let pe_header_pos = self.pe_header_pos();
		let old_header = &self.original_data.as_slice()[pe_header_pos..];

		// We make sure that the old header is not too big for us to put the linux symbols where we
		// want them
		let num_symbol_tables: [u8; 2] = old_header[0x06..0x08].try_into().unwrap();
		let num_symbol_tables = usize::from(u16::from_le_bytes(num_symbol_tables));
		let opt_section_size: [u8; 2] = old_header[0x14..0x16].try_into().unwrap();
		let opt_section_size = usize::from(u16::from_le_bytes(opt_section_size));

		let total_size_of_header: [u8; 4] = old_header[0x54..0x58].try_into().unwrap();
		let total_size_of_header =
			usize::try_from(u32::from_le_bytes(total_size_of_header)).unwrap();
		assert!(
			total_size_of_header >= LINUX_MAGIC_POS,
			"total header size is too small"
		);

		// We copy the old header, up to the max header size (start of RVA directory)
		let mut new_header = Vec::new();
		new_header.extend_from_slice(&old_header[..PE_MANDATORY_HEADER_SIZE + MAX_OPT_HEADER_SIZE]);

		if opt_section_size >= MAX_OPT_HEADER_SIZE {
			// We need to remove extraneous Header Data Directory entries.
			// Only the first six are required to boot correctly.
			// Each entry occupies 8 bytes.

			// Adapt the number of data directory entries
			// Offset: 108 in the optional header (https://learn.microsoft.com/en-us/windows/win32/debug/pe-format#optional-header-windows-specific-fields-image-only)
			new_header.as_mut_slice()
				[PE_MANDATORY_HEADER_SIZE + 108..PE_MANDATORY_HEADER_SIZE + 112]
				.copy_from_slice(6u32.to_le_bytes().as_slice());

			// Set the header size to our value
			(new_header.as_mut_slice()[0x14..0x16]).copy_from_slice(
				u16::try_from(MAX_OPT_HEADER_SIZE)
					.unwrap()
					.to_le_bytes()
					.as_slice(),
			);
		} else if opt_section_size < MAX_OPT_HEADER_SIZE {
			panic!("invalid PE header: header is too small");
		}

		let total_header_size = PE_MANDATORY_HEADER_SIZE
			+ MAX_OPT_HEADER_SIZE
			+ (num_symbol_tables * SYMBOL_TABLE_SIZE);
		assert!(
			PE_HEADER_POS + total_header_size <= LINUX_MAGIC_POS,
			"too many symbol tables! ({num_symbol_tables})"
		);

		// Copy the symbol tables
		let symbol_tables_start = PE_MANDATORY_HEADER_SIZE + opt_section_size;
		let symbol_tables_end = symbol_tables_start + (num_symbol_tables * SYMBOL_TABLE_SIZE);
		new_header.extend_from_slice(&old_header[symbol_tables_start..symbol_tables_end]);

		// Pad with zeros
		while new_header.len() + PE_HEADER_POS < LINUX_MAGIC_POS {
			new_header.push(0);
		}

		assert_eq!(new_header.len(), LINUX_MAGIC_POS - PE_HEADER_POS);

		// Linux Boot section
		new_header.extend_from_slice(b"HdrS"); // Magic string for the header
		new_header.extend_from_slice(0x20fu32.to_le_bytes().as_ref()); // Version number

		// Pad with zeros
		while new_header.len() + PE_HEADER_POS < LINUX_MAX_INITRD_SIZE_POS {
			new_header.push(0);
		}

		// Max initrd size: u32::MAX
		new_header.extend_from_slice(&[0xff; 4]);

		self.output_handle
			.write_all(&new_header)
			.expect("failed to write PE header");
	}

	fn write_pe_body(&mut self) {
		// The PE file contains basically nothing before its first object, so we can just copy everything as-is
		let position = self
			.output_handle
			.seek(SeekFrom::End(0))
			.expect("failed to obtain current position in object file");
		let position = usize::try_from(position).unwrap();

		self.output_handle
			.write_all(&self.original_data.as_slice()[position..])
			.expect("failed to write PE body")
	}

	pub fn load_from_path<T: AsRef<Path>>(path: T) -> Self {
		let contents = {
			let mut file = OpenOptions::new()
				.read(true)
				.open(path.as_ref())
				.expect("missing built object file");
			let mut contents = Vec::new();
			file.read_to_end(&mut contents)
				.expect("failed to read object file");

			contents
		};

		let write_handle = OpenOptions::new()
			.truncate(true)
			.write(true)
			.open(path.as_ref());

		Self {
			output_handle: write_handle.unwrap(),
			original_data: contents,
		}
	}

	pub fn rewrite(mut self) {
		self.write_dos_header();
		self.write_pe_header();
		self.write_pe_body();
	}
}
