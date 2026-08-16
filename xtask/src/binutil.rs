use std::path::PathBuf;
use std::sync::LazyLock;

pub fn binutil(name: &str) -> PathBuf {
	static LLVM_TOOLS: LazyLock<Option<LlvmTools>> = LazyLock::new(LlvmTools::new);

	LLVM_TOOLS
		.as_ref()
		.and_then(|llvm_tools| llvm_tools.tool(name))
		.unwrap_or(PathBuf::from(name))
}

struct LlvmTools {
	bin: PathBuf,
}

impl LlvmTools {
	pub fn new() -> Option<Self> {
		let mut rustc = crate::rustc();
		rustc.args(["--print", "sysroot"]);

		eprintln!("$ {rustc:?}");
		let output = rustc.output().unwrap();
		assert!(output.status.success());

		let sysroot = String::from_utf8(output.stdout).unwrap();
		let rustlib = [sysroot.trim_end(), "lib", "rustlib"]
			.iter()
			.collect::<PathBuf>();

		let example_exe = exe("llvm-objdump");
		for entry in rustlib.read_dir().unwrap() {
			let bin = entry.unwrap().path().join("bin");
			if bin.join(&example_exe).exists() {
				return Some(Self { bin });
			}
		}

		None
	}

	pub fn tool(&self, name: &str) -> Option<PathBuf> {
		let path = self.bin.join(exe(name));
		path.exists().then_some(path)
	}
}

fn exe(name: &str) -> String {
	let exe_suffix = std::env::consts::EXE_SUFFIX;
	format!("{name}{exe_suffix}")
}
