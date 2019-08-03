use std::env;
use std::path::Path;
use std::process::Command;

fn main() {
	let out_dir = env::var("OUT_DIR").unwrap();

	Command::new("nasm")
		.args(&["src/arch/x86_64/entry.asm", "-felf64", "-o"])
		.arg(&format!("{}/entry.o", out_dir))
		.status()
		.unwrap();
	Command::new("ar")
		.args(&["crus", "libentry.a", "entry.o"])
		.current_dir(&Path::new(&out_dir))
		.status()
		.unwrap();

	println!("cargo:rustc-link-search=native={}", out_dir);
	println!("cargo:rustc-link-lib=static=entry");
}
