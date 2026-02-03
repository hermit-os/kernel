use std::env;

fn main() {
	built::write_built_file().expect("Failed to acquire build-time information");

	set_linker_script();
}

fn set_linker_script() {
	let cfg_target_arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap();
	let cfg_feature = env::var("CARGO_CFG_FEATURE").unwrap();

	let linker_script = match cfg_target_arch.as_str() {
		"aarch64" if cfg_feature.contains("elf") => "link.ld",
		"riscv64" if cfg_feature.contains("sbi") => "link.ld",
		"x86_64" if cfg_feature.contains("linux") => "platform/linux/link.ld",
		"x86_64" if cfg_feature.contains("multiboot") => "platform/multiboot/link.ld",
		_ => return,
	};

	let linker_script = format_args!("src/arch/{cfg_target_arch}/{linker_script}");
	println!("cargo:rerun-if-changed={linker_script}");
	println!("cargo:rustc-link-arg=-T{linker_script}");
}
