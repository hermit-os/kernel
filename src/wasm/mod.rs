mod capi;

use wasmtime::*;

pub(crate) fn init() -> Result<(), Error> {
	let mut config: Config = Config::new();
	config.memory_init_cow(false);
	config.wasm_simd(false);
	config.wasm_relaxed_simd(false);

	// First step is to create the Wasm execution engine with some config.
	// In this example we are using the default configuration.
	let engine = Engine::new(&config)?;

	debug!("Create Module");
	let module_bytes = include_bytes!("fib.cwasm");
	let module = unsafe { Module::deserialize(&engine, &module_bytes[..])? };

	debug!("Create Linker");
	let linker = Linker::new(&engine);

	// All wasm objects operate within the context of a "store". Each
	// `Store` has a type parameter to store host-specific data, which in
	// this case we're using `4` for.
	let mut store = Store::new(&engine, 4);
	debug!("Create instance");
	let instance = linker.instantiate(&mut store, &module)?;

	debug!("Try to find function fibonacci");
	let fibonacci = instance.get_typed_func::<u64, u64>(&mut store, "fibonacci")?;

	// And finally we can call the wasm function
	info!("Call function fibonacci");
	let result = fibonacci.call(&mut store, 30)?;
	info!("fibonacci(30) = {}", result);
	assert!(
		result == 832040,
		"Error in the calculation of fibonacci(30) "
	);

	Ok(())
}
