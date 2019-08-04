#![feature(duration_float)]

extern crate rayon;

mod laplace;

use std::f64::consts::PI;
use std::fs::File;
use std::io::Read;
use std::io::Write;
use std::thread;
use std::time::Instant;
use std::vec;

fn bench_sched_one_thread() -> Result<(), ()> {
	let n = 1000000;

	// cache warmup
	thread::yield_now();
	thread::yield_now();
	let _now = Instant::now();

	let now = Instant::now();
	for _ in 0..n {
		thread::yield_now();
	}
	let time = now.elapsed().as_secs_f64();

	println!(
		"Scheduling time {} usec (1 thread)",
		(time * 1000000.0) / n as f64
	);

	Ok(())
}

fn bench_sched_two_threads() -> Result<(), ()> {
	let n = 1000000;
	let nthreads = 2;

	// cache warmup
	thread::yield_now();
	thread::yield_now();
	let _now = Instant::now();

	let now = Instant::now();

	let threads: Vec<_> = (0..nthreads - 1)
		.map(|_| {
			thread::spawn(move || {
				for _ in 0..n {
					thread::yield_now();
				}
			})
		})
		.collect();

	for _ in 0..n {
		thread::yield_now();
	}

	let time = now.elapsed().as_secs_f64();

	for t in threads {
		t.join().unwrap();
	}

	println!(
		"Scheduling time {} usec (2 threads)",
		(time * 1000000.0) / (nthreads * n) as f64
	);

	Ok(())
}

fn pi_sequential(num_steps: u64) -> Result<(), ()> {
	let step = 1.0 / num_steps as f64;
	let mut sum = 0 as f64;

	for i in 0..num_steps {
		let x = (i as f64 + 0.5) * step;
		sum += 4.0 / (1.0 + x * x);
	}

	let mypi = sum * (1.0 / num_steps as f64);
	println!("Pi: {} (sequential)", mypi);

	if (mypi - PI).abs() < 0.00001 {
		Ok(())
	} else {
		Err(())
	}
}

fn pi_parallel(nthreads: u64, num_steps: u64) -> Result<(), ()> {
	let step = 1.0 / num_steps as f64;
	let mut sum = 0.0 as f64;

	let threads: Vec<_> = (0..nthreads)
		.map(|tid| {
			thread::spawn(move || {
				let mut partial_sum = 0 as f64;
				let start = (num_steps / nthreads) * tid;
				let end = (num_steps / nthreads) * (tid + 1);

				for i in start..end {
					let x = (i as f64 + 0.5) * step;
					partial_sum += 4.0 / (1.0 + x * x);
				}

				partial_sum
			})
		})
		.collect();

	for t in threads {
		sum += t.join().unwrap();
	}

	let mypi = sum * (1.0 / num_steps as f64);
	println!("Pi: {} (with {} threads)", mypi, nthreads);

	if (mypi - PI).abs() < 0.00001 {
		Ok(())
	} else {
		Err(())
	}
}

fn read_file() -> Result<(), std::io::Error> {
	let mut file = File::open("/etc/hostname")?;
	let mut contents = String::new();
	file.read_to_string(&mut contents)?;

	println!("Hostname: {}", contents);

	Ok(())
}

fn create_file() -> Result<(), std::io::Error> {
	{
		let mut file = File::create("/tmp/foo.txt")?;
		file.write_all(b"Hello, world!")?;
	}

	let contents = {
		let mut file = File::open("/tmp/foo.txt")?;
		let mut contents = String::new();
		file.read_to_string(&mut contents)?;
		contents
	};

	// delete temporary file
	std::fs::remove_file("/tmp/foo.txt")?;

	if contents == "Hello, world!" {
		Ok(())
	} else {
		let kind = std::io::ErrorKind::Other;
		Err(std::io::Error::from(kind))
	}
}

fn hello() -> Result<(), ()> {
	println!("Hello, world!");

	Ok(())
}

fn threading() -> Result<(), ()> {
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

	Ok(())
}

fn test_result<T>(result: Result<(), T>) -> &'static str {
	match result {
		Ok(_) => "ok",
		Err(_) => "failed!",
	}
}

fn laplace(size_x: usize, size_y: usize) -> Result<(), ()> {
	let matrix = matrix_setup(size_x, size_y);

	let now = Instant::now();
	let (iterations, res) = laplace::compute(matrix, size_x, size_y);
	println!(
		"Time to solve {} s, iterations {}, residuum {}",
		now.elapsed().as_secs_f64(),
		iterations,
		res
	);

	if res < 0.01 {
		Ok(())
	} else {
		Err(())
	}
}

fn matrix_setup(size_x: usize, size_y: usize) -> (vec::Vec<vec::Vec<f64>>) {
	let mut matrix = vec![vec![0.0; size_x * size_y]; 2];

	// top row
	for x in 0..size_x {
		matrix[0][x] = 1.0;
		matrix[1][x] = 1.0;
	}

	// bottom row
	for x in 0..size_x {
		matrix[0][(size_y - 1) * size_x + x] = 1.0;
		matrix[1][(size_y - 1) * size_x + x] = 1.0;
	}

	// left row
	for y in 0..size_y {
		matrix[0][y * size_x] = 1.0;
		matrix[1][y * size_x] = 1.0;
	}

	// right row
	for y in 0..size_y {
		matrix[0][y * size_x + size_x - 1] = 1.0;
		matrix[1][y * size_x + size_x - 1] = 1.0;
	}

	matrix
}

fn main() {
	println!("Test {} ... {}", stringify!(hello), test_result(hello()));
	println!(
		"Test {} ... {}",
		stringify!(read_file),
		test_result(read_file())
	);
	println!(
		"Test {} ... {}",
		stringify!(create_file),
		test_result(create_file())
	);
	println!(
		"Test {} ... {}",
		stringify!(threading),
		test_result(threading())
	);
	println!(
		"Test {} ... {}",
		stringify!(pi_sequential),
		test_result(pi_sequential(50000000))
	);
	println!(
		"Test {} ... {}",
		stringify!(pi_parallel),
		test_result(pi_parallel(2, 50000000))
	);
	println!(
		"Test {} ... {}",
		stringify!(laplace),
		test_result(laplace(124, 124))
	);
	println!(
		"Test {} ... {}",
		stringify!(bench_sched_one_thread),
		test_result(bench_sched_one_thread())
	);
	println!(
		"Test {} ... {}",
		stringify!(bench_sched_two_threads),
		test_result(bench_sched_two_threads())
	);
}
