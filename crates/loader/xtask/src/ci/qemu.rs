use std::env;
use std::process::{Command, ExitStatus};

use anyhow::{Result, ensure};
use clap::Args;
use sysinfo::{CpuRefreshKind, System};
use xshell::cmd;

use crate::build::Build;
use crate::target::Target;

/// Run hermit-rs images on QEMU.
#[derive(Args)]
pub struct Qemu {
	/// Enable hardware acceleration.
	#[arg(long)]
	accel: bool,

	/// Run QEMU using `sudo`.
	#[arg(long)]
	sudo: bool,

	/// Enable the `microvm` machine type.
	#[arg(long)]
	microvm: bool,

	#[command(flatten)]
	build: Build,

	#[arg(long)]
	image: Option<String>,
}

impl Qemu {
	pub fn run(mut self) -> Result<()> {
		let default_image = if self.microvm {
			"hello_world-microvm"
		} else {
			"hello_world"
		};

		self.image.get_or_insert_with(|| default_image.to_string());

		if super::in_ci() {
			eprintln!("::group::cargo build")
		}

		self.build.run()?;

		if super::in_ci() {
			eprintln!("::endgroup::")
		}

		let sh = crate::sh()?;

		if self.build.target() == Target::X86_64Uefi {
			sh.create_dir("target/esp/efi/boot")?;
			sh.copy_file(self.build.dist_object(), "target/esp/efi/boot/bootx64.efi")?;
			sh.copy_file(
				self.build.ci_image(self.image.as_deref().unwrap()),
				"target/esp/efi/boot/hermit-app",
			)?;
		}

		let target = self.build.target();
		let arch = target.arch();
		let qemu = env::var("QEMU").unwrap_or_else(|_| format!("qemu-system-{arch}"));
		let program = if self.sudo { "sudo" } else { qemu.as_str() };
		let arg = self.sudo.then_some(qemu.as_str());

		let qemu = cmd!(sh, "{program} {arg...}")
			.args(&["-display", "none"])
			.args(&["-serial", "stdio"])
			.args(self.machine_args())
			.args(self.cpu_args())
			.args(self.memory_args());

		eprintln!("$ {qemu}");
		let status = Command::from(qemu).status()?;
		ensure!(status.qemu_success(), "QEMU exit code: {:?}", status.code());

		Ok(())
	}

	fn machine_args(&self) -> Vec<String> {
		if self.microvm {
			let frequency = get_frequency();
			vec![
				"-M".to_string(),
				"microvm,x-option-roms=off,pit=off,pic=off,rtc=on,auto-kernel-cmdline=off,acpi=off"
					.to_string(),
				"-global".to_string(),
				"virtio-mmio.force-legacy=on".to_string(),
				"-nodefaults".to_string(),
				"-no-user-config".to_string(),
				"-append".to_string(),
				format!("-freq {frequency}"),
			]
		} else if self.build.target() == Target::Aarch64 {
			vec!["-machine".to_string(), "virt,gic-version=3".to_string()]
		} else if self.build.target() == Target::Riscv64 {
			vec![
				"-machine".to_string(),
				"virt".to_string(),
				"-bios".to_string(),
				"opensbi-1.6-rv-bin/share/opensbi/lp64/generic/firmware/fw_jump.bin".to_string(),
			]
		} else {
			vec![]
		}
	}

	fn cpu_args(&self) -> Vec<String> {
		match self.build.target() {
			Target::X86_64 | Target::X86_64Uefi => {
				let mut cpu_args = if self.accel {
					if cfg!(target_os = "linux") {
						vec![
							"-enable-kvm".to_string(),
							"-cpu".to_string(),
							"host".to_string(),
						]
					} else {
						todo!()
					}
				} else {
					vec!["-cpu".to_string(), "Skylake-Client".to_string()]
				};
				cpu_args.push("-device".to_string());
				cpu_args.push("isa-debug-exit,iobase=0xf4,iosize=0x04".to_string());

				match self.build.target() {
					Target::X86_64 => {
						cpu_args.push("-kernel".to_string());
						cpu_args.push(
							self.build
								.dist_object()
								.into_os_string()
								.into_string()
								.unwrap(),
						);
						cpu_args.push("-initrd".to_string());
						cpu_args.push(
							self.build
								.ci_image(self.image.as_deref().unwrap())
								.into_os_string()
								.into_string()
								.unwrap(),
						);
					}
					Target::X86_64Uefi => {
						use ovmf_prebuilt::{Arch, FileType, Prebuilt, Source};

						let prebuilt = Prebuilt::fetch(Source::LATEST, "target/ovmf")
							.expect("failed to update prebuilt");
						let code = prebuilt.get_file(Arch::X64, FileType::Code);
						let vars = prebuilt.get_file(Arch::X64, FileType::Vars);

						cpu_args.push("-drive".to_string());
						cpu_args.push(format!(
							"if=pflash,format=raw,readonly=on,file={}",
							code.display()
						));
						cpu_args.push("-drive".to_string());
						cpu_args.push(format!(
							"if=pflash,format=raw,readonly=on,file={}",
							vars.display()
						));
						cpu_args.push("-drive".to_string());
						cpu_args.push("format=raw,file=fat:rw:target/esp".to_string());
					}
					_ => unreachable!(),
				}
				cpu_args
			}
			Target::X86_64Fc => panic!("unsupported"),
			Target::Aarch64 => {
				let mut cpu_args = if self.accel {
					todo!()
				} else {
					vec![
						"-cpu".to_string(),
						"cortex-a72".to_string(),
						"-kernel".to_string(),
						self.build
							.dist_object()
							.into_os_string()
							.into_string()
							.unwrap(),
					]
				};
				cpu_args.push("-semihosting".to_string());
				cpu_args.push("-device".to_string());
				cpu_args.push(format!(
					"guest-loader,addr=0x48000000,initrd={}",
					self.build
						.ci_image(self.image.as_deref().unwrap())
						.display()
				));
				cpu_args
			}
			Target::Riscv64 => {
				let mut cpu_args = if self.accel {
					todo!()
				} else {
					vec![
						"-cpu".to_string(),
						"rv64".to_string(),
						"-kernel".to_string(),
						self.build
							.dist_object()
							.into_os_string()
							.into_string()
							.unwrap(),
					]
				};
				cpu_args.push("-initrd".to_string());
				cpu_args.push(
					self.build
						.ci_image(self.image.as_deref().unwrap())
						.into_os_string()
						.into_string()
						.unwrap(),
				);
				cpu_args
			}
		}
	}

	fn memory(&self) -> usize {
		let mut memory = 64usize;
		match self.build.target() {
			Target::X86_64Uefi => {
				memory = memory.max(512);
			}
			Target::Aarch64 => {
				memory = memory.max(256);
			}
			Target::Riscv64 => {
				memory = memory.max(128);
			}
			_ => {}
		}
		memory
	}

	fn memory_args(&self) -> [String; 2] {
		["-m".to_string(), format!("{}M", self.memory())]
	}
}

fn get_frequency() -> u64 {
	let mut sys = System::new();
	sys.refresh_cpu_specifics(CpuRefreshKind::nothing().with_frequency());
	let frequency = sys.cpus().first().unwrap().frequency();
	if !sys.cpus().iter().all(|cpu| cpu.frequency() == frequency) {
		eprintln!("CPU frequencies are not all equal");
	}
	frequency
}

trait ExitStatusExt {
	fn qemu_success(&self) -> bool;
}

impl ExitStatusExt for ExitStatus {
	fn qemu_success(&self) -> bool {
		self.success() || self.code() == Some(3)
	}
}
