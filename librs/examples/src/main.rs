#![feature(test)]

extern crate test;

#[cfg(test)]
mod tests {
	use super::*;
	use std::thread;
	use test::Bencher;

	fn pi(nthreads: u64, num_steps: u64)
	{
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

		println!("Pi: {}", sum * (1.0 / num_steps as f64));
	}


	#[bench]
	fn bench_pi(b: &mut Bencher) {
    	b.iter(|| pi(2, 10000000));
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
