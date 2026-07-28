#![cfg_attr(not(feature = "tcp"), expect(dead_code))]

#[cfg(feature = "tcp")]
pub(crate) mod tcp;
#[cfg(feature = "udp")]
pub(crate) mod udp;
#[cfg(feature = "virtio-vsock")]
pub(crate) mod vsock;

/// Further receives will be disallowed
pub const SHUT_RD: i32 = 0;
/// Further sends will be disallowed
pub const SHUT_WR: i32 = 1;
/// Further sends and receives will be disallowed
pub const SHUT_RDWR: i32 = 2;
