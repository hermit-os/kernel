use std::net::UdpSocket;
use std::process::{Child, Command, ExitStatus};
use std::time::Duration;
use std::{env, thread};

use anyhow::{bail, ensure, Context, Result};
use clap::{Args, ValueEnum};
use sysinfo::{CpuRefreshKind, System};
use wait_timeout::ChildExt;
use xshell::cmd;

use super::build::Build;
use crate::arch::Arch;

/// Run hermit-rs images on QEMU.
#[derive(Args)]
pub struct Qemu {
	/// Enable hardware acceleration.
	#[arg(long)]
	accel: bool,

	/// Enable the `microvm` machine type.
	#[arg(long)]
	microvm: bool,

	/// Enable a network device.
	#[arg(long)]
	netdev: Option<NetworkDevice>,

	/// Create multiple vCPUs.
	#[arg(long, default_value_t = 1)]
	smp: usize,

	/// Enable the `virtiofsd` virtio-fs vhost-user device daemon.
	#[arg(long)]
	virtiofsd: bool,

	#[command(flatten)]
	build: Build,
}

#[derive(ValueEnum, Clone, Copy)]
pub enum NetworkDevice {
	VirtioNetPci,
	Rtl8139,
}

impl Qemu {
	pub fn run(mut self) -> Result<()> {
		if self.smp > 1 {
			self.build
				.cargo_build
				.features
				.push("hermit/smp".to_string());
		}

		self.build.run()?;

		let sh = crate::sh()?;

		let virtiofsd = self.virtiofsd.then(spawn_virtiofsd).transpose()?;
		thread::sleep(Duration::from_millis(100));

		let arch = self.build.cargo_build.artifact.arch.name();
		let qemu = env::var_os("QEMU").unwrap_or_else(|| format!("qemu-system-{arch}").into());

		let qemu = cmd!(sh, "{qemu}")
			.args(&["-display", "none"])
			.args(&["-serial", "stdio"])
			.args(&["-kernel", format!("hermit-loader-{arch}").as_ref()])
			.args(self.machine_args())
			.args(self.cpu_args())
			.args(&["-smp", &self.smp.to_string()])
			.args(self.memory_args())
			.args(self.netdev_args())
			.args(self.virtiofsd_args());

		eprintln!("$ {qemu}");
		let mut qemu = KillChildOnDrop(
			Command::from(qemu)
				.spawn()
				.context("Failed to spawn QEMU")?,
		);

		thread::sleep(Duration::from_millis(100));
		if let Some(status) = qemu.0.try_wait()? {
			ensure!(status.qemu_success(), "QEMU exit code: {:?}", status.code());
		}

		match self.build.package.as_str() {
			"httpd" => test_httpd()?,
			"testudp" => test_testudp()?,
			_ => {}
		}

		let status = qemu.0.wait_timeout(Duration::from_secs(60 * 2))?;
		let Some(status) = status else {
			bail!("QEMU timeout")
		};
		ensure!(status.qemu_success(), "QEMU exit code: {:?}", status.code());

		if let Some(mut virtiofsd) = virtiofsd {
			let status = virtiofsd.0.wait()?;
			assert!(status.success());
		}

		Ok(())
	}

	fn machine_args(&self) -> Vec<String> {
		if self.microvm {
			let frequency = get_frequency();
			vec![
				"-M".to_string(),
				"microvm,x-option-roms=off,pit=off,pic=off,rtc=on".to_string(),
				"-global".to_string(),
				"virtio-mmio.force-legacy=on".to_string(),
				"-nodefaults".to_string(),
				"-no-user-config".to_string(),
				"-append".to_string(),
				format!("-freq {frequency}"),
			]
		} else if self.build.cargo_build.artifact.arch == Arch::Aarch64 {
			vec!["-machine".to_string(), "virt,gic-version=3".to_string()]
		} else if self.build.cargo_build.artifact.arch == Arch::Riscv64 {
			vec!["-machine".to_string(), "virt".to_string()]
		} else {
			vec![]
		}
	}

	fn cpu_args(&self) -> Vec<String> {
		match self.build.cargo_build.artifact.arch {
			Arch::X86_64 => {
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
				cpu_args.push("-initrd".to_string());
				cpu_args.push(self.build.image().into_os_string().into_string().unwrap());
				cpu_args
			}
			Arch::Aarch64 => {
				let mut cpu_args = if self.accel {
					todo!()
				} else {
					vec!["-cpu".to_string(), "cortex-a72".to_string()]
				};
				cpu_args.push("-semihosting".to_string());
				cpu_args.push("-device".to_string());
				cpu_args.push(format!(
					"guest-loader,addr=0x48000000,initrd={}",
					self.build.image().display()
				));
				cpu_args
			}
			Arch::Riscv64 => {
				let mut cpu_args = if self.accel {
					todo!()
				} else {
					vec!["-cpu".to_string(), "rv64".to_string()]
				};
				cpu_args.push("-initrd".to_string());
				cpu_args.push(self.build.image().into_os_string().into_string().unwrap());
				cpu_args
			}
		}
	}

	fn memory(&self) -> usize {
		let mut memory = 32usize;
		if self.build.cargo_build.artifact.arch == Arch::Riscv64 {
			memory *= 2;
		}
		if self.build.cargo_build.artifact.profile() == "dev" {
			memory *= 4;
		}
		if self.netdev.is_some() {
			memory *= 4;
		}
		memory *= self.smp;
		if self.build.cargo_build.artifact.arch == Arch::Aarch64 {
			memory = memory.max(256);
		}
		memory = memory.max(64);
		memory
	}

	fn memory_args(&self) -> [String; 2] {
		["-m".to_string(), format!("{}M", self.memory())]
	}

	fn netdev_args(&self) -> &'static [&'static str] {
		match self.netdev {
			Some(NetworkDevice::VirtioNetPci) => &[
				"-netdev",
				"user,id=u1,hostfwd=tcp::9975-:9975,hostfwd=udp::9975-:9975,net=192.168.76.0/24,dhcpstart=192.168.76.9",
				"-device",
				"virtio-net-pci,netdev=u1,disable-legacy=on"
			],
			Some(NetworkDevice::Rtl8139) => &[
				"-netdev",
				"user,id=u1,hostfwd=tcp::9975-:9975,hostfwd=udp::9975-:9975,net=192.168.76.0/24,dhcpstart=192.168.76.9",
				"-device",
				"rtl8139,netdev=u1"
			],
			None => &[],
		}
	}

	fn virtiofsd_args(&self) -> Vec<String> {
		if self.virtiofsd {
			let memory = self.memory();
			vec![
				"-chardev".to_string(),
				"socket,id=char0,path=./vhostqemu".to_string(),
				"-device".to_string(),
				"vhost-user-fs-pci,queue-size=1024,chardev=char0,tag=root".to_string(),
				"-object".to_string(),
				format!("memory-backend-file,id=mem,size={memory}M,mem-path=/dev/shm,share=on"),
				"-numa".to_string(),
				"node,memdev=mem".to_string(),
			]
		} else {
			vec![]
		}
	}
}

fn spawn_virtiofsd() -> Result<KillChildOnDrop> {
	let sh = crate::sh()?;

	sh.create_dir("foo")?;

	let cmd = cmd!(sh, "virtiofsd --socket-path=./vhostqemu --shared-dir ./foo --announce-submounts --sandbox none --seccomp none --inode-file-handles=never");

	eprintln!("$ {cmd}");

	Ok(KillChildOnDrop(Command::from(cmd).spawn()?))
}

fn get_frequency() -> u64 {
	let mut sys = System::new();
	sys.refresh_cpu_specifics(CpuRefreshKind::new().with_frequency());
	let frequency = sys.cpus().first().unwrap().frequency();
	if !sys.cpus().iter().all(|cpu| cpu.frequency() == frequency) {
		eprintln!("CPU frequencies are not all equal");
	}
	frequency
}

fn test_httpd() -> Result<()> {
	thread::sleep(Duration::from_secs(10));
	eprintln!("[CI] GET http://127.0.0.1:9975");
	let body = ureq::get("http://127.0.0.1:9975")
		.timeout(Duration::from_secs(3))
		.call()?
		.into_string()?;
	eprintln!("[CI] {body}");
	assert_eq!(body.lines().next(), Some("Hello from Hermit! 🦀"));
	Ok(())
}

fn test_testudp() -> Result<()> {
	thread::sleep(Duration::from_secs(10));
	let buf = "exit";
	eprintln!("[CI] send {buf:?} via UDP to 127.0.0.1:9975");
	let socket = UdpSocket::bind("127.0.0.1:0")?;
	socket.connect("127.0.0.1:9975")?;
	socket.send(buf.as_bytes())?;
	Ok(())
}

struct KillChildOnDrop(Child);

impl Drop for KillChildOnDrop {
	fn drop(&mut self) {
		self.0.kill().ok();
	}
}

trait ExitStatusExt {
	fn qemu_success(&self) -> bool;
}

impl ExitStatusExt for ExitStatus {
	fn qemu_success(&self) -> bool {
		self.success() || self.code() == Some(3)
	}
}
