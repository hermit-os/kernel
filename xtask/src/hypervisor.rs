use std::str::FromStr;

use anyhow::anyhow;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Hypervisor {
	Uhyve,
	Qemu,
}

impl FromStr for Hypervisor {
	type Err = anyhow::Error;

	fn from_str(s: &str) -> Result<Self, Self::Err> {
		match s {
			"uhyve" => Ok(Self::Uhyve),
			"qemu" => Ok(Self::Qemu),
			s => Err(anyhow!("Unsupported hypervisor: {s}")),
		}
	}
}
