use std::f64::consts::PI;
use std::thread;

fn pi_sequential(num_steps: u64) -> bool {
	let step = 1.0 / num_steps as f64;
	let mut sum = 0 as f64;

	for i  in 0..num_steps {
		let x = (i as f64 + 0.5) * step;
		sum += 4.0 / (1.0 + x * x);
	}

	let mypi = sum * (1.0 / num_steps as f64);
	println!("Pi: {} (sequential)", mypi);

	(mypi - PI).abs() < 0.00001
}

fn pi_parallel(nthreads: u64, num_steps: u64) -> bool {
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

	(mypi - PI).abs() < 0.00001
}

fn hello() -> bool {
	println!("Hello, world!");

	true
}

fn threading() -> bool {
	// Make a vector to hold the children which are spawned.
	let mut children = vec![];

	for i in 0..2 {
		// Spin up another thread
		children.push(thread::spawn(move || {
			println!("this is thread number {}", i);
		}));
	}

	for child in children {
		// Wait for the thread to finish. Returns a result.
		let _ = child.join();
	}

	true
}

fn test_result(b: bool) -> &'static str {
	if b == true {
		"ok"
	} else {
		"failed!"
	}
}

fn main() {
	println!("Test {} ... {}", stringify!(hello), test_result(hello()));
	println!("Test {} ... {}", stringify!(threading), test_result(threading()));
	println!("Test {} ... {}", stringify!(pi_sequential), test_result(pi_sequential(50000000)));
	println!("Test {} ... {}", stringify!(pi_parallel), test_result(pi_parallel(2, 50000000)));
}
