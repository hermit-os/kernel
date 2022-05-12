use std::env;
use std::path::Path;
use std::process::Command;

fn main() {
	let target_arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap();

	if target_arch == "x86_64" {
		let out_dir = env::var("OUT_DIR").unwrap();

		Command::new("nasm")
			.args(&["src/arch/x86_64/entry.asm", "-felf64", "-o"])
			.arg(&format!("{}/entry.o", out_dir))
			.status()
			.expect("Could not start nasm. Is it installed?");
		Command::new("ar")
			.args(&["crus", "libentry.a", "entry.o"])
			.current_dir(&Path::new(&out_dir))
			.status()
			.expect("Could not start ar. Is it installed?");

		println!("cargo:rustc-link-search=native={}", out_dir);
		println!("cargo:rustc-link-lib=static=entry");
	}
}
