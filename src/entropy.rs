//! Cryptographically secure random data generation.
//!
//! This currently uses a ChaCha-based generator (the same one Linux uses!) seeded
//! with random data provided by the processor.

use hermit_sync::InterruptTicketMutex;
use rand_chacha::rand_core::{RngCore, SeedableRng};
use rand_chacha::ChaCha20Rng;

use crate::arch::kernel::processor::seed_entropy;
use crate::errno::ENOSYS;

bitflags! {
	pub struct Flags: u32 {}
}

static POOL: InterruptTicketMutex<Option<ChaCha20Rng>> = InterruptTicketMutex::new(None);

/// Fills `buf` with random data, respecting the options in `flags`.
///
/// Returns the number of bytes written or `-ENOSYS` if the system does not support
/// random data generation.
pub fn read(buf: &mut [u8], _flags: Flags) -> isize {
	let pool = &mut *POOL.lock();
	let pool = match pool {
		Some(pool) => pool,
		pool @ None => {
			if let Some(seed) = seed_entropy() {
				pool.insert(ChaCha20Rng::from_seed(seed))
			} else {
				return -ENOSYS as isize;
			}
		}
	};

	pool.fill_bytes(buf);
	// Slice lengths are always <= isize::MAX so this return value cannot conflict
	// with error numbers.
	buf.len() as isize
}
