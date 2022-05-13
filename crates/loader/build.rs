use std::env;

fn main() -> Result<(), String> {
	let target_arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap();

	if target_arch == "x86_64" {
		let mut nasm = nasm_rs::Build::new();
		nasm.file("src/arch/x86_64/entry.asm");
		let objects = nasm.compile_objects()?;

		let mut cc = cc::Build::new();
		for object in objects {
			cc.object(object);
		}
		cc.compile("entry");

		println!("cargo:rustc-link-lib=static=entry");
	}

	Ok(())
}
