extern crate target_build_utils;

use std::env;
use std::path::Path;
use std::process::Command;
use target_build_utils::TargetInfo;

fn main() {
	let target = TargetInfo::new().expect("Could not get target info");
	let out_dir = env::var("OUT_DIR").unwrap();

	if target.target_arch() == "x86_64" {
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
