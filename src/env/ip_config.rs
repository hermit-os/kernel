use core::net::Ipv4Addr;
use core::{fmt, str};

use smoltcp::wire::{IpCidr, Ipv4Cidr};

/// IP configuration as passed via ip= to the kernel commandline.
///
/// Fields are specified separated by colons (:), for example:
/// - ip=none or ip=off to skip configuring the default interface
/// - ip=dhcp to use DHCP for configuring the default interface
/// - ip=10.0.5.3/24:10.0.5.1 to configure a static IP and gateway on the default interface
///
/// This is heavily inspired by the Linux kernel's parameter of the same name:
/// <https://docs.kernel.org/admin-guide/nfs/nfsroot.html#kernel-command-line>
#[derive(Clone, Copy, Debug, Default)]
pub struct IpConfig {
	pub ip_and_gateway: IpAddrConfig,
	// hostname is omitted
	// device is omitted
	// autoconf is omitted
	#[cfg(feature = "dns")]
	pub dns0: Option<Ipv4Addr>,
	#[cfg(feature = "dns")]
	pub dns1: Option<Ipv4Addr>,
	// ntp0 is omitted
}

impl TryFrom<&str> for IpConfig {
	type Error = IpConfigParseError;

	fn try_from(value: &str) -> Result<Self, Self::Error> {
		let mut ret = Self::default();
		let mut parts = value.split(':');

		// The IP configuration is mandatory
		ret.ip_and_gateway = IpAddrConfig::parse_from_parts(&mut parts)?;

		// Everything else is optional
		let Some(_hostname) = parts.next() else {
			return Ok(ret);
		};

		let Some(_device) = parts.next() else {
			return Ok(ret);
		};

		let Some(_autoconf) = parts.next() else {
			return Ok(ret);
		};

		let Some(dns0_ip_str) = parts.next() else {
			return Ok(ret);
		};
		#[cfg(feature = "dns")]
		if !dns0_ip_str.is_empty() {
			ret.dns0 = Some(dns0_ip_str.parse().map_err(|_| Self::Error::InvalidDns)?);
		}
		#[cfg(not(feature = "dns"))]
		if !dns0_ip_str.is_empty() {
			warn!("DNS 0 IP specified without enabling the dns feature, ignoring");
		}

		let Some(dns1_ip_str) = parts.next() else {
			return Ok(ret);
		};
		#[cfg(feature = "dns")]
		if !dns1_ip_str.is_empty() {
			ret.dns1 = Some(dns1_ip_str.parse().map_err(|_| Self::Error::InvalidDns)?);
		}
		#[cfg(not(feature = "dns"))]
		if !dns1_ip_str.is_empty() {
			warn!("DNS 1 IP specified without enabling the dns feature, ignoring");
		}

		let Some(_ntp0_ip) = parts.next() else {
			return Ok(ret);
		};

		Ok(ret)
	}
}

#[derive(Debug)]
pub enum IpConfigParseError {
	#[cfg(feature = "dns")]
	InvalidDns,
	InvalidGateway,
	InvalidIp,
	InvalidPrefixLen,
	MissingIpOrMethod,
	MissingPrefixLen,
	#[cfg(not(feature = "dhcpv4"))]
	DhcpNotEnabled,
}

impl fmt::Display for IpConfigParseError {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			#[cfg(feature = "dns")]
			Self::InvalidDns => f.write_str("invalid DNS IP address"),
			Self::InvalidGateway => f.write_str("invalid gateway IP address"),
			Self::InvalidIp => f.write_str("invalid IP address"),
			Self::InvalidPrefixLen => f.write_str("invalid prefix length"),
			Self::MissingIpOrMethod => f.write_str("IP configuration is missing a method"),
			Self::MissingPrefixLen => {
				f.write_str("static IP configuration is missing a prefix length")
			}
			#[cfg(not(feature = "dhcpv4"))]
			Self::DhcpNotEnabled => f.write_str("DHCP cannot be selected, disable via feature flags"),
		}
	}
}

#[derive(Clone, Copy, Debug, Default)]
pub enum IpAddrConfig {
	#[cfg_attr(not(feature = "dhcpv4"), default)]
	None,
	#[cfg(feature = "dhcpv4")]
	#[default]
	Dhcp,
	Static {
		ip_and_netmask: IpCidr,
		gateway: Option<Ipv4Addr>,
	},
}

impl IpAddrConfig {
	fn parse_from_parts(parts: &mut str::Split<'_, char>) -> Result<Self, IpConfigParseError> {
		let ip_or_type = parts.next().ok_or(IpConfigParseError::MissingIpOrMethod)?;

		match ip_or_type {
			"none" | "off" => Ok(Self::None),
			"dhcp" => {
				#[cfg(feature = "dhcpv4")]
				{
					Ok(Self::Dhcp)
				}
				#[cfg(not(feature = "dhcpv4"))]
				{
					Err(IpConfigParseError::DhcpNotEnabled)
				}
			}
			// Anything else must be an IP with a prefix length
			ip_and_prefix => {
				let mut ip_and_prefix_parts = ip_and_prefix.split('/');
				// We only support IPv4 for now
				let ip = ip_and_prefix_parts
					.next()
					// split always has at least one item
					.unwrap()
					.parse()
					.map_err(|_| IpConfigParseError::InvalidIp)?;
				let prefix_len = ip_and_prefix_parts
					.next()
					.ok_or(IpConfigParseError::MissingPrefixLen)?
					.parse()
					.map_err(|_| IpConfigParseError::InvalidPrefixLen)?;

				// The gateway is optional since you can technically not specify one
				let gateway = parts.next().map_or(Ok(None), |ip_str| {
					ip_str
						.parse()
						.map_or(Err(IpConfigParseError::InvalidGateway), |ip| Ok(Some(ip)))
				})?;

				Ok(Self::Static {
					ip_and_netmask: IpCidr::from(Ipv4Cidr::new(ip, prefix_len)),
					gateway,
				})
			}
		}
	}
}
