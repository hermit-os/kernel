//! Cryptographically secure random data generation.
//!
//! This currently uses a ChaCha-based generator (the same one Linux uses!) seeded
//! with random data provided by the processor.

use hermit_sync::InterruptTicketMutex;
use rand_chacha::rand_core::{RngCore, SeedableRng};
use rand_chacha::ChaCha20Rng;

use crate::arch::kernel::processor::{get_timer_ticks, seed_entropy};
use crate::errno::ENOSYS;

// Reseed every second for increased security while maintaining the performance of
// the PRNG.
const RESEED_INTERVAL: u64 = 1000000;

bitflags! {
	pub struct Flags: u32 {}
}

struct Pool {
	rng: ChaCha20Rng,
	last_reseed: u64,
}

static POOL: InterruptTicketMutex<Option<Pool>> = InterruptTicketMutex::new(None);

/// Fills `buf` with random data, respecting the options in `flags`.
///
/// Returns the number of bytes written or `-ENOSYS` if the system does not support
/// random data generation.
pub fn read(buf: &mut [u8], _flags: Flags) -> isize {
	let pool = &mut *POOL.lock();
	let now = get_timer_ticks();
	let pool = match pool {
		Some(pool) if now.saturating_sub(pool.last_reseed) <= RESEED_INTERVAL => pool,
		pool => {
			if let Some(seed) = seed_entropy() {
				pool.insert(Pool {
					rng: ChaCha20Rng::from_seed(seed),
					last_reseed: now,
				})
			} else {
				return -ENOSYS as isize;
			}
		}
	};

	pool.rng.fill_bytes(buf);
	// Slice lengths are always <= isize::MAX so this return value cannot conflict
	// with error numbers.
	buf.len() as isize
}
