use std::io::{BufRead, BufReader, Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream, UdpSocket};
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
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

	/// Enable PCIe support.
	#[arg(long)]
	pci_e: bool,

	/// Enable UEFI.
	#[arg(long)]
	uefi: bool,

	/// Devices to enable.
	#[arg(long)]
	devices: Vec<Device>,

	/// Do not activate additional virtio features.
	#[arg(long)]
	no_default_virtio_features: bool,

	/// Use a TAP device for networking.
	#[arg(long)]
	tap: bool,
}

#[derive(ValueEnum, PartialEq, Eq, Clone, Copy)]
pub enum Device {
	/// Cadence Gigabit Ethernet MAC (GEM).
	CadenceGem,

	/// RTL8139.
	Rtl8139,

	/// virtio-console via MMIO.
	VirtioConsoleMmio,

	/// virtio-console via PCI.
	VirtioConsolePci,

	/// virtio-fs via MMIO.
	///
	/// This option also starts the `virtiofsd` virtio-fs vhost-user device daemon.
	VirtioFsMmio,

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
			.iter()
			.any(|device| matches!(device, Device::VirtioFsMmio | Device::VirtioFsPci))
			.then(spawn_virtiofsd)
			.transpose()?;
		thread::sleep(Duration::from_millis(100));

		let image_name = image.file_name().unwrap().to_str().unwrap();
		if image_name.contains("rftrace") {
			sh.create_dir("shared/tracedir")?;
		}

		let qemu = env::var("QEMU").unwrap_or_else(|_| format!("qemu-system-{}", arch.qemu()));
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
			.args(self.serial_args())
			.args(self.image_args(image, arch)?)
			.args(self.machine_args(arch))
			.args(self.cpu_args(arch))
			.args(&["-smp", &effective_smp.to_string()])
			.args(&["-m".to_owned(), format!("{memory}M")])
			.args(&["-global", "virtio-mmio.force-legacy=off"])
			.args(self.device_args(memory))
			.args(self.cmdline_args(image_name));

		eprintln!("$ {qemu}");
		let mut qemu = Command::from(qemu);

		if image_name == "stdin" {
			qemu.stdin(Stdio::piped()).stdout(Stdio::piped());
		}

		let mut qemu = KillChildOnDrop(qemu.spawn().context("Failed to spawn QEMU")?);

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
			"axum-example" | "http_server" | "http_server_poll" | "http_server_select" => {
				test_http_server(guest_ip)?
			}
			"httpd" => test_httpd(guest_ip)?,
			"testudp" => test_testudp(guest_ip)?,
			"miotcp" => test_miotcp(guest_ip)?,
			"mioudp" => test_mioudp(guest_ip)?,
			"poll" => test_poll(guest_ip)?,
			"stdin" => test_stdin(&mut qemu.0)?,
			_ => {}
		}

		if matches!(
			image_name,
			"axum-example" | "http_server" | "http_server_poll" | "http_server_select"
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
			sh.remove_path("target/esp")?;
			sh.create_dir("target/esp/efi/boot")?;
			sh.copy_file(loader, "target/esp/efi/boot/bootx64.efi")?;
			sh.copy_file(image, "target/esp/efi/boot/hermit-app")?;

			use ovmf_prebuilt::{Arch, FileType, Prebuilt, Source};

			let prebuilt =
				Prebuilt::fetch(Source::LATEST, "target/ovmf").expect("failed to update prebuilt");
			let code = prebuilt.get_file(Arch::X64, FileType::Code);
			let vars = prebuilt.get_file(Arch::X64, FileType::Vars);

			vec![
				"-drive".to_owned(),
				format!("if=pflash,format=raw,readonly=on,file={}", code.display()),
				"-drive".to_owned(),
				format!("if=pflash,format=raw,readonly=on,file={}", vars.display()),
				"-drive".to_owned(),
				"format=raw,file=fat:rw:target/esp".to_owned(),
			]
		} else {
			let mut image_args = vec!["-kernel".to_owned(), loader];
			match arch {
				Arch::X86_64 | Arch::Riscv64 => {
					image_args.push("-initrd".to_owned());
					image_args.push(image.to_str().unwrap().to_owned());
				}
				Arch::Aarch64 | Arch::Aarch64Be => {
					image_args.push("-device".to_owned());
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
				"-M".to_owned(),
				"microvm,x-option-roms=off,pit=off,pic=off,rtc=on,auto-kernel-cmdline=off,acpi=off"
					.to_owned(),
				"-global".to_owned(),
				"virtio-mmio.force-legacy=off".to_owned(),
				"-nodefaults".to_owned(),
				"-no-user-config".to_owned(),
			]
		} else if self.pci_e {
			vec!["-machine".to_owned(), "q35".to_owned()]
		} else if arch == Arch::Aarch64 || arch == Arch::Aarch64Be {
			vec!["-machine".to_owned(), "virt,gic-version=3".to_owned()]
		} else if arch == Arch::Riscv64 {
			// CadenceGem requires sifive_u
			let machine = if self.devices.contains(&Device::CadenceGem) {
				"sifive_u"
			} else {
				"virt"
			};
			vec![
				"-machine".to_owned(),
				machine.to_owned(),
				"-bios".to_owned(),
				"opensbi-1.7-rv-bin/share/opensbi/lp64/generic/firmware/fw_jump.bin".to_owned(),
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
							"-enable-kvm".to_owned(),
							"-cpu".to_owned(),
							"host".to_owned(),
						]
					} else {
						todo!()
					}
				} else {
					vec!["-cpu".to_owned(), "Skylake-Client".to_owned()]
				};
				cpu_args.push("-device".to_owned());
				cpu_args.push("isa-debug-exit,iobase=0xf4,iosize=0x04".to_owned());
				cpu_args
			}
			Arch::Aarch64 | Arch::Aarch64Be => {
				let mut cpu_args = if self.accel {
					todo!()
				} else {
					vec!["-cpu".to_owned(), "cortex-a72".to_owned()]
				};
				cpu_args.push("-semihosting".to_owned());
				cpu_args
			}
			Arch::Riscv64 => {
				let mut cpu_args = if self.accel {
					todo!()
				} else if self.devices.contains(&Device::CadenceGem) {
					// CadenceGem does not seem to work with rv64 as the CPU,
					// possibly because it requires sifive_u as the machine.
					vec![]
				} else {
					vec!["-cpu".to_owned(), "rv64".to_owned()]
				};
				cpu_args.push("-semihosting".to_owned());
				cpu_args
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
				Arch::Aarch64 | Arch::Aarch64Be => 144,
				Arch::Riscv64 => 86,
			};
		}

		1024
	}

	fn serial_args(&self) -> &[&str] {
		if self
			.devices
			.iter()
			.any(|device| matches!(device, Device::VirtioConsoleMmio | Device::VirtioConsolePci))
		{
			&[]
		} else {
			&["-serial", "stdio"]
		}
	}

	fn device_args(&self, memory: usize) -> Vec<String> {
		let netdev_options = if self.tap {
			"tap,id=net0,script=xtask/hermit-ifup,vhost=on"
		} else {
			"user,id=net0,hostfwd=tcp::9975-:9975,hostfwd=udp::9975-:9975,net=192.168.76.0/24,dhcpstart=192.168.76.9"
		};

		self.devices
			.iter()
			.copied()
			.flat_map(|device| match device {
				Device::CadenceGem => {
					vec![
						"-nic".to_owned(),
						format!("{netdev_options},model=cadence_gem"),
					]
				}
				device @ (Device::Rtl8139 | Device::VirtioNetMmio | Device::VirtioNetPci) => {
					let mut netdev_args = vec![
						"-netdev".to_owned(),
						netdev_options.to_owned(),
						"-device".to_owned(),
					];

					let mut device_arg = match device {
						Device::VirtioNetPci => "virtio-net-pci,netdev=net0,disable-legacy=on",
						Device::VirtioNetMmio => "virtio-net-device,netdev=net0",
						Device::Rtl8139 => "rtl8139,netdev=net0",
						_ => unreachable!(),
					}
					.to_owned();

					if !self.no_default_virtio_features
						&& (device == Device::VirtioNetPci || device == Device::VirtioNetMmio)
					{
						device_arg.push_str(",packed=on,mq=on");
					}

					netdev_args.push(device_arg);

					netdev_args
				}
				device @ (Device::VirtioFsMmio | Device::VirtioFsPci) => {
					let device_arg = match device {
						Device::VirtioFsMmio => "vhost-user-fs-device",
						Device::VirtioFsPci => "vhost-user-fs-pci",
						_ => unreachable!(),
					};
					let default_virtio_features = if !self.no_default_virtio_features {
						",packed=on"
					} else {
						""
					};
					vec![
						"-chardev".to_owned(),
						"socket,id=char0,path=./vhostqemu".to_owned(),
						"-device".to_owned(),
						format!(
							"{device_arg},queue-size=1024{default_virtio_features},chardev=char0,tag=root"
						),
						"-object".to_owned(),
						format!(
							"memory-backend-file,id=mem,size={memory}M,mem-path=/dev/shm,share=on"
						),
						"-numa".to_owned(),
						"node,memdev=mem".to_owned(),
					]
				}
				device @ (Device::VirtioConsoleMmio | Device::VirtioConsolePci) => {
					let device_arg = match device {
						Device::VirtioConsoleMmio => "virtio-serial-device",
						Device::VirtioConsolePci => "virtio-serial-pci,disable-legacy=on",
						_ => unreachable!(),
					};

					vec![
						"-chardev".to_owned(),
						"stdio,id=char0,mux=on".to_owned(),
						"-serial".to_owned(),
						"chardev:char0".to_owned(),
						"-device".to_owned(),
						device_arg.to_owned(),
						"-device".to_owned(),
						"virtconsole,chardev=char0".to_owned(),
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
		if self.tap {
			if let Ok(ip) = env::var("HERMIT_IP") {
				ip.parse().unwrap()
			} else {
				Ipv4Addr::new(10, 0, 5, 3).into()
			}
		} else {
			Ipv4Addr::LOCALHOST.into()
		}
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

fn test_stdin(child: &mut Child) -> Result<()> {
	thread::sleep(Duration::from_secs(10));
	let messages = ["Hello, there!", "Hello, again!", "Bye-bye!"];

	let mut stdin = child.stdin.take().unwrap();
	for message in messages {
		writeln!(&mut stdin, "{message}")?;
		stdin.flush()?;
		thread::sleep(Duration::from_secs(1));
	}

	child.kill()?;

	let stdout = child.stdout.take().unwrap();
	let stdout_lines = BufReader::new(stdout)
		.lines()
		.collect::<Result<Vec<_>, _>>()?;

	for line in &stdout_lines {
		println!("{line}");
	}

	for message in messages {
		assert!(stdout_lines.iter().any(|line| line.contains(message)));
	}

	Ok(())
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
	let symbols = cmd!(sh, "{nm} --demangle --numeric-sort {image}")
		.output()?
		.stdout;
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
