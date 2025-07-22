use alloc::collections::VecDeque;
use alloc::vec::Vec;
use core::hint::black_box;
use core::mem::MaybeUninit;

use hermit_sync::{InterruptTicketMutex, without_interrupts};
use wasi::*;
use wasmtime::*;
use zerocopy::IntoBytes;

use crate::kernel::systemtime::now_micros;
use crate::syscalls::sys_write;

mod capi;

#[inline(never)]
fn native_fibonacci(n: u64) -> u64 {
	match n {
		0 => 0,
		1 => 1,
		_ => native_fibonacci(n - 1) + native_fibonacci(n - 2),
	}
}

pub fn measure_fibonacci(n: u64) {
	const RUNS: u64 = 100;
	info!("Measure native_fibonacci({})", n);

	let start = now_micros();
	for _ in 0..RUNS {
		black_box(native_fibonacci(black_box(n)));
	}
	let end = now_micros();
	info!(
		"Average time to call native_fibonacci({}): {} usec",
		n,
		(end - start) / RUNS
	);
}

pub(crate) static WASM_MANAGER: InterruptTicketMutex<Option<WasmManager>> =
	InterruptTicketMutex::new(None);
pub(crate) static INPUT: InterruptTicketMutex<VecDeque<Vec<u8>>> =
	InterruptTicketMutex::new(VecDeque::new());

pub(crate) struct WasmManager {
	store: Store<u32>,
	instance: Instance,
}

impl WasmManager {
	pub fn new(slice: &[u8]) -> Self {
		let mut config: Config = Config::new();
		config.memory_init_cow(false);
		config.memory_guard_size(8192);
		config.wasm_simd(false);
		config.wasm_relaxed_simd(false);
		//config.wasm_reference_types(true);
		//config.wasm_gc(true);

		let engine = Engine::new(&config).unwrap();
		let module = unsafe { Module::deserialize(&engine, slice).unwrap() };
		let mut linker = Linker::new(&engine);
		linker
			.func_wrap("env", "now", || {
				crate::arch::kernel::systemtime::now_micros()
			})
			.unwrap();
		linker
			.func_wrap(
				"wasi_snapshot_preview1",
				"fd_read",
				|mut caller: Caller<'_, _>,
				 fd: i32,
				 iovs_ptr: i32,
				 iovs_len: i32,
				 nread_ptr: i32| {
					without_interrupts(|| {
						if let Some(Extern::Memory(mem)) = caller.get_export("memory") {
							info!("A");
							let mut iovs = vec![0i32; (2 * iovs_len).try_into().unwrap()];
							let _ = mem.read(
								caller.as_context(),
								iovs_ptr.try_into().unwrap(),
								iovs.as_mut_bytes(),
							);

							info!("B");
							let mut nread_bytes: i32 = 0;
							let mut i = 0;
							//if let Some(data) = INPUT.lock().pop_front() {
							info!("iovs.len() = {}, {}", iovs.len(), 0); //data.len());

							/*while i < iovs.len() {
								let _ = mem.write(
									caller.as_context_mut(),
									iovs[i].try_into().unwrap(),
									&data,
								);

								nread_bytes += data.len() as i32;
								//if result < len.try_into().unwrap() {
									break;
								//}

								i += 2;
							}*/
							//}

							let _ = mem.write(
								caller.as_context_mut(),
								nread_ptr.try_into().unwrap(),
								nread_bytes.as_bytes(),
							);

							return ERRNO_SUCCESS.raw() as i32;
						}

						ERRNO_INVAL.raw() as i32
					})
				},
			)
			.unwrap();
		linker
			.func_wrap(
				"wasi_snapshot_preview1",
				"fd_write",
				|mut caller: Caller<'_, u32>,
				 fd: i32,
				 iovs_ptr: i32,
				 iovs_len: i32,
				 nwritten_ptr: i32| {
					let fd = if fd <= 2 {
						fd
					} else {
						panic!("fd_write: invalid file descriptor {}", fd);
					};

					info!("fd_write: fd = {}", fd);
					if let Some(Extern::Memory(mem)) = caller.get_export("memory") {
						let mut iovs = vec![0i32; (2 * iovs_len).try_into().unwrap()];
						let _ = mem.read(
							caller.as_context(),
							iovs_ptr.try_into().unwrap(),
							iovs.as_mut_bytes(),
						);

						let mut nwritten_bytes: i32 = 0;
						let mut i = 0;
						info!("iovs.len() = {}", iovs.len());
						while i < iovs.len() {
							let len = iovs[i + 1];

							info!("len = {}", len);
							// len = 0 => ignore entry nothing to write
							if len == 0 {
								i += 2;
								continue;
							}

							let mut data: Vec<MaybeUninit<u8>> =
								Vec::with_capacity(len.try_into().unwrap());
							unsafe {
								data.set_len(len as usize);
							}

							let _ = mem.read(
								caller.as_context(),
								iovs[i].try_into().unwrap(),
								unsafe { data.assume_init_mut() },
							);
							let result = unsafe {
								sys_write(
									fd,
									data.assume_init_ref().as_ptr(),
									len.try_into().unwrap(),
								)
							};

							info!("fd_write: result = {}", result);

							if result >= 0 {
								nwritten_bytes += result as i32;
								info!("nwritten_bytes = {}, len = {}", nwritten_bytes, len);
								if result < len.try_into().unwrap() {
									info!("break");
									break;
								}
							} else {
								return (-result).try_into().unwrap();
							}

							i += 2;
						}

						info!("JJ");
						let _ = mem.write(
							caller.as_context_mut(),
							nwritten_ptr.try_into().unwrap(),
							nwritten_bytes.as_bytes(),
						);

						info!("KK");
						return ERRNO_SUCCESS.raw() as i32;
					}

					ERRNO_INVAL.raw() as i32
				},
			)
			.unwrap();
		linker
			.func_wrap(
				"wasi_snapshot_preview1",
				"environ_get",
				|mut caller: Caller<'_, _>, env_ptr: i32, env_buffer_ptr: i32| {
					if let Some(Extern::Memory(mem)) = caller.get_export("memory") {
						let mut _pos: u32 = env_buffer_ptr as u32;
					}
					ERRNO_SUCCESS.raw() as i32
				},
			)
			.unwrap();
		linker
			.func_wrap(
				"wasi_snapshot_preview1",
				"environ_sizes_get",
				|mut caller: Caller<'_, _>,
				 number_env_variables_ptr: i32,
				 env_buffer_size_ptr: i32| {
					if let Some(Extern::Memory(mem)) = caller.get_export("memory") {
						let env_buffer_size: u32 = 0;
						let nnumber_env_variables: u32 = 0;

						let _ = mem.write(
							caller.as_context_mut(),
							number_env_variables_ptr.try_into().unwrap(),
							nnumber_env_variables.as_bytes(),
						);
						let _ = mem.write(
							caller.as_context_mut(),
							env_buffer_size_ptr.try_into().unwrap(),
							env_buffer_size.as_bytes(),
						);

						return ERRNO_SUCCESS.raw() as i32;
					}

					ERRNO_INVAL.raw() as i32
				},
			)
			.unwrap();
		linker
			.func_wrap("wasi_snapshot_preview1", "proc_exit", |_: i32| {
				error!("Panic in WASM module")
			})
			.unwrap();

		// All wasm objects operate within the context of a "store". Each
		// `Store` has a type parameter to store host-specific data, which in
		// this case we're using `4` for.
		let mut store = Store::new(&engine, 4);
		let instance = linker.instantiate(&mut store, &module).unwrap();

		let func = instance
			.get_typed_func::<(), ()>(&mut store, "hello_world")
			.unwrap();
		func.call(&mut store, ()).unwrap();

		Self { store, instance }
	}

	pub fn call_func<P, R>(&mut self, name: &str, arg: P) -> Result<R>
	where
		P: wasmtime::WasmParams,
		R: wasmtime::WasmResults,
	{
		let func = self
			.instance
			.get_typed_func::<P, R>(&mut self.store, name)?;
		func.call(&mut self.store, arg)
	}
}
#[hermit_macro::system]
#[unsafe(no_mangle)]
pub extern "C" fn sys_load_binary(ptr: *const u8, len: usize) -> i32 {
	info!("Loading WebAssembly binary...");

	// copy module into the kernel space
	let slice = unsafe { core::slice::from_raw_parts(ptr, len) };
	let wasm_manager = WasmManager::new(slice);

	*WASM_MANAGER.lock() = Some(wasm_manager);

	if let Some(ref mut wasm_manager) = WASM_MANAGER.lock().as_mut() {
		info!("Call function fibonacci");
		let result = wasm_manager.call_func::<u64, u64>("fibonacci", 30).unwrap();
		info!("fibonacci(30) = {}", result);

		wasm_manager.call_func::<(), ()>("hello_world", ()).unwrap();
	}

	0
}

#[hermit_macro::system]
#[unsafe(no_mangle)]
pub extern "C" fn sys_fibonacci() -> i32 {
	info!("Try to find function fibonacci");

	measure_fibonacci(30);

	if let Some(ref mut wasm_manager) = WASM_MANAGER.lock().as_mut() {
		// And finally we can call the wasm function
		info!("Call function fibonacci");
		let result = wasm_manager.call_func::<u64, u64>("fibonacci", 30).unwrap();
		info!("fibonacci(30) = {}", result);
		assert!(
			result == 832040,
			"Error in the calculation of fibonacci(30) "
		);

		const RUNS: u64 = 100;
		let n = 30;
		let start = now_micros();
		for _ in 0..RUNS {
			black_box(wasm_manager.call_func::<u64, u64>("fibonacci", n).unwrap());
		}
		let end = now_micros();
		info!(
			"Average time to call fibonacci({}) in WASM: {} usec",
			n,
			(end - start) / RUNS
		);

		info!(
			"Average time to call measure_fibonacci({}) in WASM: {} usec",
			n,
			wasm_manager
				.call_func::<u64, u64>("measure_fibonacci", n)
				.unwrap()
		);
	}

	0
}
