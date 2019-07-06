use std::f64::consts::PI;
use std::fs::File;
use std::io::Read;
use std::io::Write;
use std::thread;

type Result<T> = std::result::Result<T, ()>;

fn pi_sequential(num_steps: u64) -> Result<()> {
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

fn pi_parallel(nthreads: u64, num_steps: u64) -> Result<()> {
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

fn read_file() -> Result<()> {
    let mut file = File::open("/etc/hostname").unwrap();
    let mut contents = String::new();
    file.read_to_string(&mut contents).unwrap();

    println!("Hostname: {}", contents);

    Ok(())
}

fn create_file() -> Result<()> {
    {
        let mut file = File::create("/tmp/foo.txt").unwrap();
        file.write_all(b"Hello, world!").unwrap();
    }

    let mut file = File::open("/tmp/foo.txt").unwrap();
    let mut contents = String::new();
    file.read_to_string(&mut contents).unwrap();

    if contents == "Hello, world!" {
        Ok(())
    } else {
        Err(())
    }
}

fn hello() -> Result<()> {
    println!("Hello, world!");

    Ok(())
}

fn threading() -> Result<()> {
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

fn test_result(result: Result<()>) -> &'static str {
    match result {
        Ok(_) => "ok",
        Err(_) => "failed!",
    }
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
}
