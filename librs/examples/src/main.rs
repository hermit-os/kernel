#![feature(test)]

extern crate test;

use std::f64::consts::PI;

#[cfg(test)]
mod tests {
	use super::*;
	use std::thread;
	use test::Bencher;

	fn pi_sequentiel(num_steps: u64) -> f64 {
		let step = 1.0 / num_steps as f64;
		let mut sum = 0 as f64;

		for i  in 0..num_steps {
			let x = (i as f64 + 0.5) * step;
			sum += 4.0 / (1.0 + x * x);
		}

		let mypi = sum * (1.0 / num_steps as f64);
		println!("Pi: {} (sequentiell)", mypi);

		mypi
	}

	fn pi_parallel(nthreads: u64, num_steps: u64) -> f64 {
		let step = 1.0 / num_steps as f64;
		let mut sum = 0.0 as f64;

		let threads: Vec<_> = (0..nthreads)
		.map(|tid| {
			thread::spawn(move || {
				let mut partial_sum = 0 as f64;
				let start = (num_steps / nthreads) * tid;
				let end = (num_steps / nthreads) * (tid+1);

				for i  in start..end {
					let x = (i as f64 + 0.5) * step;
					partial_sum += 4.0 / (1.0 + x * x);
				}

				partial_sum
			})
		}).collect();

		for t in threads {
			sum += t.join().unwrap();
		}

		let mypi = sum * (1.0 / num_steps as f64);
		println!("Pi: {} (with {} threads)", mypi, nthreads);

		mypi
	}


	#[bench]
	fn bench_pi_sequentiel(b: &mut Bencher) {
		b.iter(|| pi_sequentiel(50000000));
	}

	#[test]
	fn pi_sequentiel_test() {
		let mypi = pi_sequentiel(50000000);

		assert!((mypi - PI).abs() < 0.00001);
	}

	#[test]
	fn hello() {
		println!("Hello, world!");
	}
}

fn main() {
	println!("Hello from HermitCore!");
	println!("Please use `cargo test --no-run --target x86_64-unknown-hermit' to build HermitCore tests");
}
