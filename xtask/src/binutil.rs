use std::io;
use std::path::PathBuf;
use std::sync::LazyLock;

pub fn binutil(name: &str) -> Option<PathBuf> {
	static LLVM_TOOLS: LazyLock<LlvmTools> = LazyLock::new(|| LlvmTools::new().unwrap());

	LLVM_TOOLS.tool(name)
}

struct LlvmTools {
	bin: PathBuf,
}

impl LlvmTools {
	pub fn new() -> io::Result<Self> {
		let mut rustc = crate::rustc();
		rustc.args(["--print", "sysroot"]);

		eprintln!("$ {rustc:?}");
		let output = rustc.output()?;
		assert!(output.status.success());

		let sysroot = String::from_utf8(output.stdout).unwrap();
		let rustlib = [sysroot.trim_end(), "lib", "rustlib"]
			.iter()
			.collect::<PathBuf>();

		let example_exe = exe("objdump");
		for entry in rustlib.read_dir()? {
			let bin = entry?.path().join("bin");
			if bin.join(&example_exe).exists() {
				return Ok(Self { bin });
			}
		}
		Err(io::Error::new(
			io::ErrorKind::NotFound,
			"Could not find llvm-tools component\n\
			\n\
			Maybe the rustup component `llvm-tools` is missing? Install it through: `rustup component add llvm-tools`",
		))
	}

	pub fn tool(&self, name: &str) -> Option<PathBuf> {
		let path = self.bin.join(exe(name));
		path.exists().then_some(path)
	}
}

fn exe(name: &str) -> String {
	let exe_suffix = std::env::consts::EXE_SUFFIX;
	format!("llvm-{name}{exe_suffix}")
}
