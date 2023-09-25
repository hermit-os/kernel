use std::env;

fn main() -> Result<(), String> {
	let target_arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap();
	let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap();
	let fc = env::var_os("CARGO_FEATURE_FC").is_some();

	if target_arch == "x86_64" && target_os == "none" {
		let mut nasm = nasm_rs::Build::new();

		let entry = if fc {
			"src/arch/x86_64/entry_fc.asm"
		} else {
			"src/arch/x86_64/entry.asm"
		};
		nasm.file(entry);
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
