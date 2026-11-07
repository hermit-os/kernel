use core::fmt;
use core::str::FromStr;

/// A virtio-mmio device as passed via `virtio_mmio.device=` to the kernel commandline.
///
/// The format is `<size>@<baseaddr>:<irq>[:<id>]`, as in Linux:
/// <https://docs.kernel.org/admin-guide/kernel-parameters.html>
///
/// The size is validated but not kept, since the registers fit into the page at `base`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VirtioMmioDevice {
	pub base: u64,
	pub irq: u8,
}

#[derive(Debug, PartialEq, Eq)]
pub struct VirtioMmioParseError;

impl fmt::Display for VirtioMmioParseError {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_str("expected <size>@<baseaddr>:<irq>[:<id>]")
	}
}

/// Parses a decimal or `0x`-prefixed hexadecimal number.
fn parse_int(s: &str) -> Option<u64> {
	match s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
		Some(hex) => u64::from_str_radix(hex, 16).ok(),
		None => s.parse().ok(),
	}
}

/// Parses a size with an optional `K`, `M` or `G` suffix, like Linux' `memparse`.
fn parse_size(s: &str) -> Option<u64> {
	let (digits, shift) = match s.as_bytes().last()? {
		b'K' | b'k' => (&s[..s.len() - 1], 10),
		b'M' | b'm' => (&s[..s.len() - 1], 20),
		b'G' | b'g' => (&s[..s.len() - 1], 30),
		_ => (s, 0),
	};
	parse_int(digits)?.checked_mul(1 << shift)
}

impl FromStr for VirtioMmioDevice {
	type Err = VirtioMmioParseError;

	fn from_str(s: &str) -> Result<Self, Self::Err> {
		let s = s.trim().trim_matches('\0');
		let (size, rest) = s.split_once('@').ok_or(VirtioMmioParseError)?;
		let mut fields = rest.split(':');
		let base = fields.next().ok_or(VirtioMmioParseError)?;
		let irq = fields.next().ok_or(VirtioMmioParseError)?;
		let id = fields.next();
		if fields.next().is_some() || id.is_some_and(|id| id.parse::<i32>().is_err()) {
			return Err(VirtioMmioParseError);
		}

		parse_size(size)
			.filter(|&size| size > 0)
			.ok_or(VirtioMmioParseError)?;
		Ok(Self {
			base: parse_int(base).ok_or(VirtioMmioParseError)?,
			irq: irq
				.parse()
				.ok()
				.filter(|&irq| irq != 0)
				.ok_or(VirtioMmioParseError)?,
		})
	}
}

#[cfg(all(test, not(target_os = "none")))]
mod tests {
	use super::*;

	fn parse(s: &str) -> Result<VirtioMmioDevice, VirtioMmioParseError> {
		s.parse()
	}

	fn device(base: u64, irq: u8) -> Result<VirtioMmioDevice, VirtioMmioParseError> {
		Ok(VirtioMmioDevice { base, irq })
	}

	#[test]
	fn firecracker() {
		assert_eq!(parse("4K@0xc0001000:5"), device(0xc000_1000, 5));
		assert_eq!(parse("4K@0xc0001000:5\0"), device(0xc000_1000, 5));
	}

	#[test]
	fn qemu_microvm() {
		assert_eq!(parse("512@0xfeb00e00:12"), device(0xfeb0_0e00, 12));
	}

	#[test]
	fn sizes() {
		assert_eq!(parse_size("512"), Some(512));
		assert_eq!(parse_size("0x200"), Some(0x200));
		assert_eq!(parse_size("4K"), Some(0x1000));
		assert_eq!(parse_size("4k"), Some(0x1000));
		assert_eq!(parse_size("1M"), Some(0x10_0000));
		assert_eq!(parse_size("2G"), Some(0x8000_0000));
		assert_eq!(parse_size("0x10K"), Some(0x4000));
		assert_eq!(parse_size("K"), None);
		assert_eq!(parse("0x200@0x1000:1"), device(0x1000, 1));
	}

	#[test]
	fn decimal_base() {
		assert_eq!(parse("512@4096:3"), device(4096, 3));
	}

	#[test]
	fn platform_device_id() {
		assert_eq!(parse("512@0xfeb00000:5:3"), device(0xfeb0_0000, 5));
		assert_eq!(parse("512@0xfeb00000:5:x"), Err(VirtioMmioParseError));
	}

	#[test]
	fn invalid() {
		for s in [
			"",
			"4K",
			"4K@",
			"4K@0xd0000000",
			"4K@0xd0000000:",
			"@0xd0000000:5",
			"0@0xd0000000:5",
			"4X@0xd0000000:5",
			"4K@0xzz:5",
			"4K@0xd0000000:0",
			"4K@0xd0000000:256",
			"4K@0xd0000000:5:1:2",
			"99999999999G@0xd0000000:5",
		] {
			assert_eq!(parse(s), Err(VirtioMmioParseError), "{s:?}");
		}
	}
}
