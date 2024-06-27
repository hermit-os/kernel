#![allow(dependency_on_unit_never_type_fallback)]
mod capi;

use wasmtime::*;

use crate::kernel::systemtime::now_micros;

#[inline(never)]
pub fn native_fibonacci(n: u64) -> u64 {
	match n {
		0 => 0,
		1 => 1,
		_ => native_fibonacci(n - 1) + native_fibonacci(n - 2),
	}
}

pub(crate) fn init() -> Result<(), Error> {
	let mut config: Config = Config::new();
	config.memory_init_cow(false);
	config.wasm_simd(false);
	config.wasm_relaxed_simd(false);

	// First step is to create the Wasm execution engine with some config.
	// In this example we are using the default configuration.
	let engine = Engine::new(&config)?;

	info!("Create Module");
	let module_bytes = include_bytes!("fib.cwasm");
	let module = unsafe { Module::deserialize(&engine, &module_bytes[..])? };

	let mut imports = module.imports();
	while let Some(i) = imports.next() {
		info!("import from module {} symbol {}", i.module(), i.name());
	}

	info!("Create Linker");
	let mut linker = Linker::new(&engine);

	// In case WASI, it is required to emulate
	// https://github.com/WebAssembly/WASI/blob/main/legacy/preview1/docs.md

	linker.func_wrap("env", "now", || {
		crate::arch::kernel::systemtime::now_micros()
	})?;
	linker.func_wrap("env", "exit", || panic!("Panic in WASM module"))?;

	// All wasm objects operate within the context of a "store". Each
	// `Store` has a type parameter to store host-specific data, which in
	// this case we're using `4` for.
	let mut store = Store::new(&engine, 4);
	info!("Create instance");
	let instance = linker.instantiate(&mut store, &module).unwrap();

	info!("Try to find function fibonacci");
	let fibonacci = instance.get_typed_func::<u64, u64>(&mut store, "fibonacci")?;

	// And finally we can call the wasm function
	info!("Call function fibonacci");
	let result = fibonacci.call(&mut store, 30)?;
	info!("fibonacci(30) = {}", result);
	assert!(
		result == 832040,
		"Error in the calculation of fibonacci(30) "
	);

	const N: u64 = 100;
	let start = now_micros();
	for _ in 0..N {
		let _result = fibonacci.call(&mut store, 30)?;
	}
	let end = now_micros();
	info!(
		"Average time to call fibonacci(30): {} usec",
		(end - start) / N
	);

	let start = now_micros();
	for _ in 0..N {
		let _result = native_fibonacci(30);
	}
	let end = now_micros();
	info!(
		"Average time to call native_fibonacci(30): {} usec",
		(end - start) / N
	);

	let bench = instance.get_typed_func::<(u64, u64), i64>(&mut store, "bench")?;
	let usec = bench.call(&mut store, (N, 30))?;
	info!("Benchmark takes {} msec", usec / 1000);

	let foo = instance.get_typed_func::<(), ()>(&mut store, "foo")?;
	foo.call(&mut store, ())?;
	let start = now_micros();
	for _ in 0..N {
		foo.call(&mut store, ())?;
	}
	let end = now_micros();
	info!("Average time to call foo: {} usec", (end - start) / N);

	Ok(())
}
