//! Cryptographically secure random data generation.
//!
//! This currently uses a [fast-key-erasure construction] around a ChaCha20-based
//! random number generator (the same one Linux uses!). Seeds come from either
//! the VirtIO RNG device or from the on-chip RNG. Reseeding after every second
//! to minimize the time required for recovery from state compromise to occur.
//!
//! [fast-key-erasure construction]: https://blog.cr.yp.to/20170723-random.html

use hermit_sync::InterruptTicketMutex;
use rand_chacha::ChaCha20Rng;
use rand_chacha::rand_core::{Rng, SeedableRng};
use zeroize::Zeroize;

use crate::arch::kernel::processor::{get_timer_ticks, seed_entropy as processor_seed_entropy};
#[cfg(feature = "virtio-rng")]
use crate::drivers::rng::seed_entropy as virtio_seed_entropy;
use crate::errno::Errno;
use crate::io;

// Reseed every second for increased security while maintaining the performance of
// the PRNG.
const RESEED_INTERVAL: u64 = 1_000_000;

// Bernstein uses this output size as an example, it's certainly a reasonable choice
// even though we use ChaCha20 instead of AES.
const POOL_SIZE: usize = 736;
const SEED_SIZE: usize = 32;

bitflags! {
	pub struct Flags: u32 {}
}

struct Pool {
	// The first SEED_SIZE bytes contain the next ChaCha20 seed, `self.available`
	// bytes after that are usable as output.
	seed_pool: [u8; SEED_SIZE + POOL_SIZE],
	block_pos: u64,
	available: usize,
	last_reseed: u64,
}

impl Pool {
	fn reseed(&mut self, seed: &[u8; SEED_SIZE]) {
		let mut rng = ChaCha20Rng::from_seed(*seed);
		// FIXME: switch to chacha20 and use `set_block_pos`.
		rng.set_word_pos(u128::from(self.block_pos) * 16);
		// Generate the next seed and POOL_SIZE bytes of output.
		rng.fill_bytes(&mut self.seed_pool);
		// Carry over the block position so that we don't get stuck in a cycle
		// in the incredibly unlikely case that the first SEED_SIZE bytes of the
		// output are identical to the seed.
		// FIXME: switch to chacha20 and use `set_block_pos`.
		self.block_pos = (rng.get_word_pos() / 16) as u64;
		self.available = POOL_SIZE;

		// FIXME: use the chacha20 crate once it can build with SIMD disabled.
		// SAFETY: `ChaCha20Rng` doesn't have a drop implementation and thus
		//         isn't accessed from this point onwards.
		unsafe { zeroize::zeroize_flat_type(&raw mut rng) };
	}

	fn fill_bytes(&mut self, mut bytes: &mut [u8]) {
		while !bytes.is_empty() {
			if self.available > 0 {
				let len = usize::min(self.available, bytes.len());
				// Take the output from the end of the pool for simplicity.
				let pool_bytes = &mut self.seed_pool[SEED_SIZE + self.available - len..][..len];
				bytes[..len].copy_from_slice(pool_bytes);
				// Since the seed used to generate the pool has been overwritten,
				// one cannot reconstruct these bytes by looking at the pool
				// once they are erased.
				pool_bytes.zeroize();
				self.available -= len;
				bytes = &mut bytes[len..];
			} else {
				let mut seed = self.seed_pool[..SEED_SIZE].try_into().unwrap();
				self.reseed(&seed);
				seed.zeroize();
			}
		}
	}
}

static POOL: InterruptTicketMutex<Option<Pool>> = InterruptTicketMutex::new(None);

/// Fills `buf` with random data, respecting the options in `flags`.
///
/// Returns the number of bytes written or `-ENOSYS` if the system does not support
/// random data generation.
pub fn read(buf: &mut [u8], _flags: Flags) -> io::Result<usize> {
	let pool = &mut *POOL.lock();
	let now = get_timer_ticks();
	let pool = match pool {
		// FIXME: detect changes to the VM generation ID to guard against VM forks.
		Some(pool) if now.saturating_sub(pool.last_reseed) <= RESEED_INTERVAL => pool,
		Some(pool) => {
			// If the underlying RNG becomes unavailable, continue to generate
			// bytes with the existing state – the only benefit of reseeding
			// is recovery from state compromise. However, we'll continue
			// to make requests on every call until the RNG comes back so
			// that recovery will happen as soon as the RNG becomes available.
			if let Some(mut seed) = seed_entropy() {
				pool.reseed(&seed);
				pool.last_reseed = now;
				seed.zeroize();
			}

			pool
		}
		None => {
			// Use let-else instead of `ok_or` to decrease the amount of seed
			// copies, we must try very hard to make sure that the seed is erased.
			let Some(mut seed) = seed_entropy() else {
				// We don't have a seed yet, so there's no choice but to fail.
				// `rand_jitter` could help in that case, but its a risky choice
				// in the environments that VMs run in. Users should always
				// configure the VirtIO RNG device.
				return Err(Errno::Nosys);
			};
			let pool = pool.insert(Pool {
				seed_pool: [0; SEED_SIZE + POOL_SIZE],
				block_pos: 0,
				available: 0,
				last_reseed: now,
			});
			pool.reseed(&seed);
			seed.zeroize();
			pool
		}
	};

	match buf.len() {
		// We do pool initialization even for zero-sized requests so that
		// we can preserve the guarantee that if `read_entropy` succeeds once,
		// it will continue to work.
		0 => {}
		// Fill small requests from the pool.
		1..=POOL_SIZE => pool.fill_bytes(buf),
		// For very large requests, only draw a seed from the pool and expand
		// that. This avoids temporary copies to the pool and its rekeying
		// behavior, which is redundant in this case (since forward-secrecy
		// is not important for a single request).
		_ => {
			let mut seed = [0u8; SEED_SIZE];
			pool.fill_bytes(&mut seed);
			let mut rng = ChaCha20Rng::from_seed(seed);
			rng.fill_bytes(buf);
			seed.zeroize();
			// FIXME: use the ChaCha20 crate once it can build with SIMD disabled.
			// SAFETY: `ChaCha20Rng` doesn't have a drop implementation and thus
			//         isn't accessed from this point onwards.
			unsafe { zeroize::zeroize_flat_type(&raw mut rng) };
		}
	}

	// Slice lengths are always <= isize::MAX so this return value cannot conflict
	// with error numbers.
	Ok(buf.len())
}

fn seed_entropy() -> Option<[u8; 32]> {
	// Prefer the VirtIO RNG device over the processor RNG. The trustworthiness
	// of RDSEED and friends is questionable, both due to hardware bugs and
	// potential backdoors.
	//
	// NOTE(joboet): Actually these hardware bugs provide some evidence *against*
	// there being a backdoor (at least a backdoor of a more trivial kind), as
	// they are very consistent with what you'd expect from faulty hardware.
	#[cfg(feature = "virtio-rng")]
	if let Some(entropy) = virtio_seed_entropy() {
		return Some(entropy);
	}

	processor_seed_entropy()
}
