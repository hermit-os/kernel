use std::process::Command;

fn main() {
	// create boot code for application processors
	/*let _ = Command::new("nasm").args(&["-f", "bin", "-o", "src/arch/x86_64/kernel/boot.bin", "src/arch/x86_64/kernel/boot.asm"]).output().unwrap();
	let _ = Command::new("sh").args(&["-c", "echo -n \"pub static SMP_BOOT_CODE: [u8; \" > src/arch/x86_64/kernel/smp_boot_code.rs"]).output().unwrap();
	let _ = Command::new("sh").args(&["-c", "stat -c %s src/arch/x86_64/kernel/boot.bin >> src/arch/x86_64/kernel/smp_boot_code.rs"]).output().unwrap();
	let _ = Command::new("sh").args(&["-c", "echo -n \"] = [\" >>  src/arch/x86_64/kernel/smp_boot_code.rs"]).output().unwrap();
	let _ = Command::new("sh").args(&["-c", "hexdump -v -e \"1/1 \\\"0x%02X,\\\"\" src/arch/x86_64/kernel/boot.bin >> src/arch/x86_64/kernel/smp_boot_code.rs"]).output().unwrap();
	let _ = Command::new("sh").args(&["-c", "echo -n \"];\" >> src/arch/x86_64/kernel/smp_boot_code.rs"]).output().unwrap();
	// build pci ids as rust file
	let _ =	Command::new("pci_ids_parser").args(&["src/arch/x86_64/kernel/pci.ids", "src/arch/x86_64/kernel/pci_ids.rs"]).output().unwrap();*/
}
