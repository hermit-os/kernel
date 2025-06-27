use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream, UdpSocket};
use std::path::Path;
use std::process::{Child, Command, ExitStatus};
use std::str::from_utf8;
use std::time::Duration;
use std::{env, fs, thread};

use anyhow::{Context, Result, bail, ensure};
use clap::{Args, ValueEnum};
use sysinfo::{CpuRefreshKind, System};
use wait_timeout::ChildExt;
use xshell::cmd;

use crate::arch::Arch;

/// Run image on QEMU.
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

	/// Enable UEFI.
	#[arg(long)]
	uefi: bool,

	/// Devices to enable.
	#[arg(long)]
	devices: Vec<Device>,

	/// Do not activate additional virtio features.
	#[arg(long)]
	no_default_virtio_features: bool,
}

#[derive(ValueEnum, PartialEq, Eq, Clone, Copy)]
pub enum Device {
	/// Cadence Gigabit Ethernet MAC (GEM).
	CadenceGem,

	/// RTL8139.
	Rtl8139,

	/// virtio-fs via PCI.
	///
	/// This option also starts the `virtiofsd` virtio-fs vhost-user device daemon.
	VirtioFsPci,

	/// virtio-net via MMIO.
	VirtioNetMmio,

	/// virtio-net via PCI.
	VirtioNetPci,
}

impl Qemu {
	pub fn run(self, image: &Path, smp: usize, arch: Arch, small: bool) -> Result<()> {
		let sh = crate::sh()?;

		let virtiofsd = self
			.devices
			.contains(&Device::VirtioFsPci)
			.then(spawn_virtiofsd)
			.transpose()?;
		thread::sleep(Duration::from_millis(100));

		let image_name = image.file_name().unwrap().to_str().unwrap();
		if image_name.contains("rftrace") {
			sh.create_dir("shared/tracedir")?;
		}

		let qemu = env::var("QEMU").unwrap_or_else(|_| format!("qemu-system-{arch}"));
		let program = if self.sudo { "sudo" } else { qemu.as_str() };
		let arg = self.sudo.then_some(qemu.as_str());
		let memory = self.memory(image_name, arch, small);

		// CadenceGem requires sifive_u, which in turn requires an SMP of at least 2.
		let effective_smp = if self.devices.contains(&Device::CadenceGem) {
			usize::max(smp, 2)
		} else {
			smp
		};

		let qemu = cmd!(sh, "{program} {arg...}")
			.args(&["-display", "none"])
			.args(&["-serial", "stdio"])
			.args(self.image_args(image, arch)?)
			.args(self.machine_args(arch))
			.args(self.cpu_args(arch))
			.args(&["-smp", &effective_smp.to_string()])
			.args(&["-m".to_string(), format!("{memory}M")])
			.args(&["-global", "virtio-mmio.force-legacy=off"])
			.args(self.device_args(memory))
			.args(self.cmdline_args(image_name));

		eprintln!("$ {qemu}");
		let mut qemu = KillChildOnDrop(
			Command::from(qemu)
				.spawn()
				.context("Failed to spawn QEMU")?,
		);

		thread::sleep(Duration::from_millis(100));
		if let Some(status) = qemu.0.try_wait()? {
			ensure!(
				self.qemu_success(status, arch),
				"QEMU exit code: {:?}",
				status.code()
			);
		}

		let guest_ip = self.guest_ip();

		match image_name {
			"axum-example" | "http_server" | "http_server_poll" => test_http_server(guest_ip)?,
			"httpd" => test_httpd(guest_ip)?,
			"testudp" => test_testudp(guest_ip)?,
			"miotcp" => test_miotcp(guest_ip)?,
			"mioudp" => test_mioudp(guest_ip)?,
			"poll" => test_poll(guest_ip)?,
			_ => {}
		}

		if matches!(
			image_name,
			"axum-example" | "http_server" | "http_server_poll"
		) || self.devices.contains(&Device::CadenceGem)
		// sifive_u, on which we test CadenceGem, does not support software shutdowns, so we have to kill the machine ourselves.
		{
			qemu.0.kill()?;
		}

		let status = qemu.0.wait_timeout(Duration::from_secs(60 * 6))?;
		let Some(status) = status else {
			bail!("QEMU timeout")
		};
		ensure!(
			self.qemu_success(status, arch),
			"QEMU exit code: {:?}",
			status.code()
		);

		if let Some(mut virtiofsd) = virtiofsd {
			let status = virtiofsd.0.wait()?;
			assert!(status.success());
		}

		if image_name.contains("rftrace") {
			check_rftrace(image)?;
		}

		Ok(())
	}

	fn image_args(&self, image: &Path, arch: Arch) -> Result<Vec<String>> {
		let exe_suffix = if self.uefi { ".efi" } else { "" };
		let loader = format!("hermit-loader-{arch}{exe_suffix}");

		let image_args = if self.uefi {
			let sh = crate::sh()?;
			sh.create_dir("target/esp/efi/boot")?;
			sh.copy_file(loader, "target/esp/efi/boot/bootx64.efi")?;
			sh.copy_file(image, "target/esp/efi/boot/hermit-app")?;

			use ovmf_prebuilt::{Arch, FileType, Prebuilt, Source};

			let prebuilt =
				Prebuilt::fetch(Source::LATEST, "target/ovmf").expect("failed to update prebuilt");
			let code = prebuilt.get_file(Arch::X64, FileType::Code);
			let vars = prebuilt.get_file(Arch::X64, FileType::Vars);

			vec![
				"-drive".to_string(),
				format!("if=pflash,format=raw,readonly=on,file={}", code.display()),
				"-drive".to_string(),
				format!("if=pflash,format=raw,readonly=on,file={}", vars.display()),
				"-drive".to_string(),
				"format=raw,file=fat:rw:target/esp".to_string(),
			]
		} else {
			let mut image_args = vec!["-kernel".to_string(), loader];
			match arch {
				Arch::X86_64 | Arch::Riscv64 => {
					image_args.push("-initrd".to_string());
					image_args.push(image.to_str().unwrap().to_string());
				}
				Arch::Aarch64 => {
					image_args.push("-device".to_string());
					image_args.push(format!(
						"guest-loader,addr=0x48000000,initrd={}",
						image.display()
					));
				}
			}
			image_args
		};

		Ok(image_args)
	}

	fn machine_args(&self, arch: Arch) -> Vec<String> {
		if self.microvm {
			vec![
				"-M".to_string(),
				"microvm,x-option-roms=off,pit=off,pic=off,rtc=on,auto-kernel-cmdline=off,acpi=off"
					.to_string(),
				"-global".to_string(),
				"virtio-mmio.force-legacy=off".to_string(),
				"-nodefaults".to_string(),
				"-no-user-config".to_string(),
			]
		} else if arch == Arch::Aarch64 {
			vec!["-machine".to_string(), "virt,gic-version=3".to_string()]
		} else if arch == Arch::Riscv64 {
			// CadenceGem requires sifive_u
			let machine = if self.devices.contains(&Device::CadenceGem) {
				"sifive_u"
			} else {
				"virt"
			};
			vec![
				"-machine".to_string(),
				machine.to_string(),
				"-bios".to_string(),
				"opensbi-1.6-rv-bin/share/opensbi/lp64/generic/firmware/fw_jump.bin".to_string(),
			]
		} else {
			vec![]
		}
	}

	fn cpu_args(&self, arch: Arch) -> Vec<String> {
		match arch {
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
				cpu_args
			}
			Arch::Aarch64 => {
				let mut cpu_args = if self.accel {
					todo!()
				} else {
					vec!["-cpu".to_string(), "cortex-a72".to_string()]
				};
				cpu_args.push("-semihosting".to_string());
				cpu_args
			}
			Arch::Riscv64 => {
				if self.accel {
					todo!()
				} else if self.devices.contains(&Device::CadenceGem) {
					// CadenceGem does not seem to work with rv64 as the CPU,
					// possibly because it requires sifive_u as the machine.
					vec![]
				} else {
					vec!["-cpu".to_string(), "rv64".to_string()]
				}
			}
		}
	}

	fn memory(&self, image_name: &str, arch: Arch, small: bool) -> usize {
		if small && image_name == "hello_world" {
			return match arch {
				Arch::X86_64 => {
					if self.uefi {
						64
					} else {
						32
					}
				}
				Arch::Aarch64 => 144,
				Arch::Riscv64 => 40,
			};
		}

		1024
	}

	fn device_args(&self, memory: usize) -> Vec<String> {
		const NETDEV_OPTIONS: &str = "user,id=u1,hostfwd=tcp::9975-:9975,hostfwd=udp::9975-:9975,net=192.168.76.0/24,dhcpstart=192.168.76.9";

		self.devices
			.iter()
			.copied()
			.flat_map(|device| match device {
				Device::CadenceGem => {
					vec![
						"-nic".to_string(),
						format!("{NETDEV_OPTIONS},model=cadence_gem"),
					]
				}
				device @ (Device::Rtl8139 | Device::VirtioNetMmio | Device::VirtioNetPci) => {
					let mut netdev_args = vec![
						"-netdev".to_string(),
						NETDEV_OPTIONS.to_string(),
						"-device".to_string(),
					];

					let mut device_arg = match device {
						Device::VirtioNetPci => "virtio-net-pci,netdev=u1,disable-legacy=on",
						Device::VirtioNetMmio => "virtio-net-device,netdev=u1",
						Device::Rtl8139 => "rtl8139,netdev=u1",
						Device::CadenceGem | Device::VirtioFsPci => unreachable!(),
					}
					.to_string();

					if !self.no_default_virtio_features
						&& (device == Device::VirtioNetPci || device == Device::VirtioNetMmio)
					{
						device_arg.push_str(",packed=on,mq=on");
					}

					netdev_args.push(device_arg);

					netdev_args
				}
				Device::VirtioFsPci => {
					let default_virtio_features = if !self.no_default_virtio_features {
						",packed=on"
					} else {
						""
					};
					vec![
						"-chardev".to_string(),
						"socket,id=char0,path=./vhostqemu".to_string(),
						"-device".to_string(),
						format!(
							"vhost-user-fs-pci,queue-size=1024{default_virtio_features},chardev=char0,tag=root"
						),
						"-object".to_string(),
						format!(
							"memory-backend-file,id=mem,size={memory}M,mem-path=/dev/shm,share=on"
						),
						"-numa".to_string(),
						"node,memdev=mem".to_string(),
					]
				}
			})
			.collect()
	}

	fn cmdline_args(&self, image_name: &str) -> Vec<String> {
		let mut cmdline = self.kernel_args();

		let mut app_args = self.app_args(image_name);
		if !app_args.is_empty() {
			cmdline.push("--".to_owned());
			cmdline.append(&mut app_args);
		}

		if cmdline.is_empty() {
			return vec![];
		}

		vec!["-append".to_owned(), cmdline.join(" ")]
	}

	fn kernel_args(&self) -> Vec<String> {
		if self.microvm {
			let frequency = get_frequency();
			vec!["-freq".to_owned(), frequency.to_string()]
		} else {
			vec![]
		}
	}

	fn app_args(&self, image_name: &str) -> Vec<String> {
		match image_name {
			"hermit-wasm" => vec!["/root/hello_world.wasm".to_owned()],
			_ => vec![],
		}
	}

	fn qemu_success(&self, status: ExitStatus, arch: Arch) -> bool {
		if status.code().is_none() {
			return true;
		}

		if arch == Arch::X86_64 {
			status.code() == Some(3)
		} else {
			status.success()
		}
	}

	fn guest_ip(&self) -> IpAddr {
		Ipv4Addr::LOCALHOST.into()
	}
}

fn spawn_virtiofsd() -> Result<KillChildOnDrop> {
	let sh = crate::sh()?;

	sh.create_dir("shared")?;

	let cmd = cmd!(
		sh,
		"virtiofsd --socket-path=./vhostqemu --shared-dir ./shared --announce-submounts --sandbox none --seccomp none --inode-file-handles=never"
	);

	eprintln!("$ {cmd}");

	Ok(KillChildOnDrop(Command::from(cmd).spawn()?))
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

fn test_http_server(guest_ip: IpAddr) -> Result<()> {
	thread::sleep(Duration::from_secs(10));
	let url = format!("http://{guest_ip}:9975");
	eprintln!("[CI] GET {url}");
	let body = ureq::get(url)
		.config()
		.timeout_global(Some(Duration::from_secs(3)))
		.build()
		.call()?
		.into_body()
		.read_to_string()?;
	eprintln!("[CI] body = {body:?}");
	assert_eq!(body, "Hello, world!\n");
	Ok(())
}

fn test_httpd(guest_ip: IpAddr) -> Result<()> {
	thread::sleep(Duration::from_secs(10));
	let url = format!("http://{guest_ip}:9975");
	eprintln!("[CI] GET {url}");
	let body = ureq::get(url)
		.config()
		.timeout_global(Some(Duration::from_secs(3)))
		.build()
		.call()?
		.into_body()
		.read_to_string()?;
	eprintln!("[CI] {body}");
	assert_eq!(body.lines().next(), Some("Hello from Hermit! 🦀"));
	Ok(())
}

fn test_testudp(guest_ip: IpAddr) -> Result<()> {
	thread::sleep(Duration::from_secs(10));
	let buf = "exit";
	let socket_addr = SocketAddr::new(guest_ip, 9975);
	eprintln!("[CI] send {buf:?} via UDP to {socket_addr}");
	let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))?;
	socket.connect(socket_addr)?;
	socket.send(buf.as_bytes())?;

	Ok(())
}

fn test_miotcp(guest_ip: IpAddr) -> Result<()> {
	thread::sleep(Duration::from_secs(10));
	let buf = "exit";
	let socket_addr = SocketAddr::new(guest_ip, 9975);
	eprintln!("[CI] send {buf:?} via TCP to {socket_addr}");
	let mut stream = TcpStream::connect(socket_addr)?;
	stream.write_all(buf.as_bytes())?;

	let mut buf = vec![];
	let received = stream.read_to_end(&mut buf)?;
	eprintln!("[CI] receive: {}", from_utf8(&buf[..received])?);

	Ok(())
}

fn test_poll(guest_ip: IpAddr) -> Result<()> {
	thread::sleep(Duration::from_secs(10));
	let buf = "exit";
	let socket_addr = SocketAddr::new(guest_ip, 9975);
	eprintln!("[CI] send {buf:?} via TCP to {socket_addr}");
	let mut stream = TcpStream::connect(socket_addr)?;
	stream.write_all(buf.as_bytes())?;

	let mut buf = vec![];
	let received = stream.read_to_end(&mut buf)?;
	eprintln!("[CI] receive: {}", from_utf8(&buf[..received])?);

	Ok(())
}

fn test_mioudp(guest_ip: IpAddr) -> Result<()> {
	thread::sleep(Duration::from_secs(10));
	let buf = "exit";
	let socket_addr = SocketAddr::new(guest_ip, 9975);
	eprintln!("[CI] send {buf:?} via UDP to {socket_addr}");
	let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))?;
	socket.connect(socket_addr)?;
	socket.send(buf.as_bytes())?;

	socket.set_read_timeout(Some(Duration::from_secs(10)))?;
	let mut buf = [0; 128];
	let received = socket.recv(&mut buf)?;
	eprintln!("[CI] receive: {}", from_utf8(&buf[..received])?);

	Ok(())
}

fn check_rftrace(image: &Path) -> Result<()> {
	let sh = crate::sh()?;
	let image_name = image.file_name().unwrap().to_str().unwrap();

	let nm = crate::binutil("nm").unwrap();
	let symbols = cmd!(sh, "{nm} --numeric-sort {image}").output()?.stdout;
	sh.write_file(format!("shared/tracedir/{image_name}.sym"), symbols)?;

	let replay = cmd!(
		sh,
		"uftrace replay --data=shared/tracedir --output-fields=tid"
	)
	.read()?;
	eprintln!("[CI] replay: {replay}");

	let expected = fs::read_to_string("xtask/src/ci/rftrace.snap")?;
	if !replay.starts_with(&expected) {
		eprintln!("[CI] expected: {expected}");
		bail!("rftrace output does not match snapshot");
	}

	eprintln!("[CI] replay matches snapshot");

	Ok(())
}

struct KillChildOnDrop(Child);

impl Drop for KillChildOnDrop {
	fn drop(&mut self) {
		self.0.kill().ok();
	}
}
