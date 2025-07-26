use alloc::collections::VecDeque;
use alloc::vec::Vec;
use core::future;
use core::hint::black_box;
use core::mem::MaybeUninit;
use core::task::Poll;

use hermit_sync::{InterruptTicketMutex, Lazy};
use wasi::*;
use wasmtime::*;
use zerocopy::IntoBytes;

use crate::executor::{WakerRegistration, spawn};
use crate::fd;
use crate::kernel::systemtime::now_micros;

mod capi;

#[inline(never)]
fn native_fibonacci(n: u64) -> u64 {
	match n {
		0 => 0,
		1 => 1,
		_ => native_fibonacci(n - 1) + native_fibonacci(n - 2),
	}
}

#[inline(never)]
fn native_foo() {}

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
static OUTPUT: InterruptTicketMutex<WasmStdout> = InterruptTicketMutex::new(WasmStdout::new());

struct WasmStdout {
	pub data: VecDeque<Vec<u8>>,
	pub waker: WakerRegistration,
}

impl WasmStdout {
	pub const fn new() -> Self {
		Self {
			data: VecDeque::new(),
			waker: WakerRegistration::new(),
		}
	}

	pub fn write(&mut self, buf: &[u8]) {
		self.data.push_back(buf.to_vec());
		self.waker.wake();
	}
}

pub(crate) struct WasmManager {
	store: Store<u32>,
	instance: Instance,
}

impl WasmManager {
	pub fn new(data: &[u8]) -> Self {
		static MODULE_AND_ARGS: Lazy<Vec<&[u8]>> = Lazy::new(|| vec![b"dummy\0"]);
		let mut config: Config = Config::new();
		config.memory_init_cow(false);
		config.memory_guard_size(8192);
		config.wasm_simd(false);
		config.wasm_relaxed_simd(false);

		let engine = Engine::new(&config).unwrap();
		let module = unsafe { Module::deserialize(&engine, data).unwrap() };
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
					let _fd = if fd <= 2 {
						fd
					} else {
						panic!("fd_read: invalid file descriptor {}", fd);
					};

					if let Some(Extern::Memory(mem)) = caller.get_export("memory") {
						let mut iovs = vec![0i32; (2 * iovs_len).try_into().unwrap()];
						let _ = mem.read(
							caller.as_context(),
							iovs_ptr.try_into().unwrap(),
							iovs.as_mut_bytes(),
						);

						let mut nread_bytes: i32 = 0;
						let i = 0;
						if let Some(data) = INPUT.lock().pop_front() {
							let _len = iovs[i + 1];

							if !data.is_empty() {
								let _ = mem.write(
									caller.as_context_mut(),
									iovs[i].try_into().unwrap(),
									&data,
								);

								nread_bytes += data.len() as i32;
							}
						}

						let _ = mem.write(
							caller.as_context_mut(),
							nread_ptr.try_into().unwrap(),
							nread_bytes.as_bytes(),
						);

						return ERRNO_SUCCESS.raw() as i32;
					}

					ERRNO_INVAL.raw() as i32
				},
			)
			.unwrap();
		linker
			.func_wrap(
				"wasi_snapshot_preview1",
				"fd_write",
				|mut caller: Caller<'_, u32>,
				 _fd: i32,
				 iovs_ptr: i32,
				 iovs_len: i32,
				 nwritten_ptr: i32| {
					if let Some(Extern::Memory(mem)) = caller.get_export("memory") {
						let mut iovs = vec![0i32; (2 * iovs_len).try_into().unwrap()];
						let _ = mem.read(
							caller.as_context(),
							iovs_ptr.try_into().unwrap(),
							iovs.as_mut_bytes(),
						);

						let mut nwritten_bytes: i32 = 0;
						let mut i = 0;
						while i < iovs.len() {
							let len = iovs[i + 1];

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
							OUTPUT.lock().write(unsafe { data.assume_init_mut() });
							nwritten_bytes += len;

							i += 2;
						}

						let _ = mem.write(
							caller.as_context_mut(),
							nwritten_ptr.try_into().unwrap(),
							nwritten_bytes.as_bytes(),
						);

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
				|mut _caller: Caller<'_, _>, _env_ptr: i32, _env_buffer_ptr: i32| {
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
			.func_wrap(
				"wasi_snapshot_preview1",
				"args_get",
				|mut caller: Caller<'_, _>, argv_ptr: i32, argv_buf_ptr: i32| {
					if let Some(Extern::Memory(mem)) = caller.get_export("memory") {
						let mut pos: u32 = argv_buf_ptr as u32;
						for (i, element) in MODULE_AND_ARGS.iter().enumerate() {
							let _ = mem.write(
								caller.as_context_mut(),
								(argv_ptr + (i * size_of::<u32>()) as i32)
									.try_into()
									.unwrap(),
								pos.as_bytes(),
							);

							let _ = mem.write(
								caller.as_context_mut(),
								pos.try_into().unwrap(),
								element,
							);

							pos += element.len() as u32;
						}
					}
					ERRNO_SUCCESS.raw() as i32
				},
			)
			.unwrap();
		linker
			.func_wrap(
				"wasi_snapshot_preview1",
				"args_sizes_get",
				move |mut caller: Caller<'_, _>, number_args_ptr: i32, args_size_ptr: i32| {
					let nargs: u32 = MODULE_AND_ARGS.len().try_into().unwrap();
					// Currently, we ignore the arguments
					if let Some(Extern::Memory(mem)) = caller.get_export("memory") {
						let _ = mem.write(
							caller.as_context_mut(),
							number_args_ptr.try_into().unwrap(),
							nargs.as_bytes(),
						);

						let nargs_size: u32 = MODULE_AND_ARGS
							.iter()
							.fold(0, |acc, arg| acc + arg.len())
							.try_into()
							.unwrap();
						let _ = mem.write(
							caller.as_context_mut(),
							args_size_ptr.try_into().unwrap(),
							nargs_size.as_bytes(),
						);

						return ERRNO_SUCCESS.raw() as i32;
					}

					ERRNO_INVAL.raw() as i32
				},
			)
			.unwrap();
		linker
			.func_wrap(
				"wasi_snapshot_preview1",
				"clock_time_get",
				|mut caller: Caller<'_, _>, clock_id: i32, _precision: i64, timestamp_ptr: i32| {
					match clock_id {
						0 => {
							let usec = crate::arch::kernel::systemtime::now_micros();
							if let Some(Extern::Memory(mem)) = caller.get_export("memory") {
								let nanos = usec * 1000;
								let _ = mem.write(
									caller.as_context_mut(),
									timestamp_ptr.try_into().unwrap(),
									nanos.as_bytes(),
								);

								return ERRNO_SUCCESS.raw() as i32;
							}

							ERRNO_INVAL.raw() as i32
						}
						1 => {
							warn!("Unsupported clock_id");
							ERRNO_INVAL.raw() as i32
						}
						_ => ERRNO_INVAL.raw() as i32,
					}
				},
			)
			.unwrap();
		linker
			.func_wrap("wasi_snapshot_preview1", "fd_close", |_fd: i32| {
				ERRNO_SUCCESS.raw() as i32
			})
			.unwrap();
		linker
			.func_wrap(
				"wasi_snapshot_preview1",
				"fd_fdstat_get",
				|_: i32, _: i32| {
					warn!("Unsupported function fd_fdstat_get");
					ERRNO_SUCCESS.raw() as i32
				},
			)
			.unwrap();
		linker
			.func_wrap(
				"wasi_snapshot_preview1",
				"fd_seek",
				|_: i32, _: i64, _: i32, _: i32| {
					warn!("Unsupported function fd_seek");
					ERRNO_SUCCESS.raw() as i32
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
pub extern "C" fn sys_unload_binary() -> i32 {
	*WASM_MANAGER.lock() = None;

	0
}

async fn wasm_run() {
	loop {
		let obj = crate::core_scheduler()
			.get_object(fd::STDOUT_FILENO)
			.unwrap();

		while let Some(data) = OUTPUT.lock().data.pop_front() {
			obj.write(&data).await.unwrap();
		}

		future::poll_fn(|cx| {
			let mut guard = OUTPUT.lock();
			if guard.data.is_empty() {
				guard.waker.register(cx.waker());
				Poll::Pending
			} else {
				Poll::Ready(())
			}
		})
		.await;
	}
}

#[hermit_macro::system]
#[unsafe(no_mangle)]
pub extern "C" fn sys_load_binary(ptr: *const u8, len: usize) -> i32 {
	info!("Loading WebAssembly binary...");

	let start = now_micros();
	let slice = unsafe { core::slice::from_raw_parts(ptr, len) };
	let wasm_manager = WasmManager::new(slice);

	*WASM_MANAGER.lock() = Some(wasm_manager);
	let end = now_micros();
	info!("Time to initiate WASM module {} usec", end - start);

	if let Some(ref mut wasm_manager) = crate::wasm::WASM_MANAGER.lock().as_mut() {
		let _ = wasm_manager.call_func::<(), ()>("hello_world", ());
	}

	spawn(wasm_run());

	0
}

#[hermit_macro::system]
#[unsafe(no_mangle)]
pub extern "C" fn sys_dhrystone() -> i32 {
	if let Some(ref mut wasm_manager) = WASM_MANAGER.lock().as_mut() {
		// And finally we can call the wasm function
		info!("Call function dhrystone");
		let _result = wasm_manager.call_func::<(), ()>("_start", ()).unwrap();
	}

	0
}

#[hermit_macro::system]
#[unsafe(no_mangle)]
pub extern "C" fn sys_foo() -> i32 {
	if let Some(ref mut wasm_manager) = WASM_MANAGER.lock().as_mut() {
		// And finally we can call the wasm function
		info!("Call function foo");
		let _result = wasm_manager.call_func::<(), ()>("foo", ()).unwrap();

		const RUNS: u64 = 1000000;
		let start = now_micros();
		for _ in 0..RUNS {
			black_box(wasm_manager.call_func::<(), ()>("foo", ()).unwrap());
		}
		let end = now_micros();
		info!(
			"Average time to call the WASM function foo: {} nsec",
			(1000 * (end - start)) / RUNS
		);

		let start = now_micros();
		for _ in 0..RUNS {
			black_box(native_foo());
		}
		let end = now_micros();

		info!(
			"Time to call {} times the function foo: {} nsec",
			RUNS,
			1000 * (end - start)
		);
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
