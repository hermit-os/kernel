mod gdt;
mod page_tables;
mod stack;

use core::ptr::NonNull;
use core::sync::atomic::{AtomicU32, Ordering};

use hermit_entry::boot_info::{BootInfo, HardwareInfo, LoadInfo, PlatformInfo, SerialPortBase, TlsInfo};
use pvh::start_info::reader::{IdentityMap, StartInfoReader};

use self::stack::{STACK, Stack};
use crate::env::setboot_info2;
use crate::kernel::pre_init;

/// The PVH entry point.
#[unsafe(naked)]
pub(crate) unsafe extern "C" fn pvh_start32() -> ! {
	core::arch::naked_asm!(
		".code32",
		include_str!("pvh_start32.s"),
		".code64",

		level_4_table = sym page_tables::LEVEL_4_TABLE,
		gdt_ptr = sym gdt::GDT_PTR,
		kernel_data_selector = const gdt::Gdt::kernel_data_selector().0,

		stack = sym STACK,
		stack_size = const size_of::<Stack>(),
		kernel_code_selector = const gdt::Gdt::kernel_code_selector().0,
		rust_start = sym rust_start,
	);
}

pvh::xen_elfnote_phys32_entry!(pvh_start32);

/// The native ELF entry point.
#[unsafe(no_mangle)]
#[unsafe(naked)]
unsafe extern "C" fn _start() -> ! {
	core::arch::naked_asm!("2: jmp 2b");
}

static START_INFO_PADDR: AtomicU32 = AtomicU32::new(0);

pub fn start_info<'a>() -> StartInfoReader<'a, IdentityMap> {
	let paddr = START_INFO_PADDR.load(Ordering::Relaxed);
	unsafe { StartInfoReader::from_paddr_identity(paddr).unwrap() }
}

/// The Rust entry point.
unsafe extern "C" fn rust_start(start_info_paddr: u32) -> ! {
	START_INFO_PADDR.store(start_info_paddr, Ordering::Relaxed);

	let start_info = start_info();

	println!("Start info:\n{start_info:#?}");

	dbg!(tdata());

	let boot_info = BootInfo {
		hardware_info: HardwareInfo {
			phys_addr_range: 0..0,
			serial_port_base: SerialPortBase::new(0x3f8),
			device_tree: None,
		},
		load_info: LoadInfo {
			kernel_image_addr_range: executable_start() as u64..executable_end() as u64,
			tls_info: dbg!(find_tls_ranges()),
		},
		platform_info: PlatformInfo::Fdt,
	};

	println!("boot_info = {boot_info:#x?}");

	setboot_info2(boot_info);

	unsafe { pre_init(None, 0) }
}

pub fn executable_start() -> *mut () {
	unsafe extern "C" {
		static mut __executable_start: u8;
	}

	(&raw mut __executable_start).cast::<()>()
}

pub fn executable_end() -> *mut () {
	unsafe extern "C" {
		static mut _end: u8;
	}

	(&raw mut _end).cast::<()>()
}

pub fn tdata() -> *mut () {
	unsafe extern "C" {
		static mut __ehdr_start: u8;
	}

	(&raw mut __ehdr_start).cast::<()>()
}


use core::slice;

const PT_LOAD: u32 = 1;
const PT_TLS:  u32 = 7;

#[repr(C)]
struct Elf64Ehdr {
    e_ident: [u8; 16],
    e_type: u16,
    e_machine: u16,
    e_version: u32,
    e_entry: u64,
    e_phoff: u64,
    e_shoff: u64,
    e_flags: u32,
    e_ehsize: u16,
    e_phentsize: u16,
    e_phnum: u16,
    e_shentsize: u16,
    e_shnum: u16,
    e_shstrndx: u16,
}

#[repr(C)]
struct Elf64Phdr {
    p_type: u32,
    p_flags: u32,
    p_offset: u64,
    p_vaddr: u64,
    p_paddr: u64,
    p_filesz: u64,
    p_memsz: u64,
    p_align: u64,
}

// Provided by the default ELF linker scripts for executables
unsafe extern "C" {
    static __ehdr_start: Elf64Ehdr;
}

#[derive(Debug, Clone, Copy)]
pub struct TlsRanges {
    pub tdata_start: *const u8,
    pub tdata_end:   *const u8,
    pub tbss_start:  *const u8,
    pub tbss_end:    *const u8,
}

unsafe fn find_tls_ranges() -> Option<TlsInfo> {
    let ehdr = &__ehdr_start as *const Elf64Ehdr;
    let ehdr_ref = &*ehdr;

    // Optional: check ELF magic
    if ehdr_ref.e_ident[0..4] != [0x7f, b'E', b'L', b'F'] {
        return None;
    }

    // Program headers
    let phdr_ptr = (ehdr as *const u8).add(ehdr_ref.e_phoff as usize) as *const Elf64Phdr;
    let phdrs = slice::from_raw_parts(phdr_ptr, ehdr_ref.e_phnum as usize);

    // Find PT_LOAD segment that contains file offset 0 (usually p_offset == 0)
    let first_load = phdrs
        .iter()
        .find(|ph| ph.p_type == PT_LOAD && ph.p_offset == 0)?
        ;

    // Compute load base of the main executable
    let ehdr_addr = &__ehdr_start as *const _ as usize;
    let base = ehdr_addr.wrapping_sub(first_load.p_vaddr as usize);

    // Find PT_TLS segment
    let tls_ph = phdrs.iter().find(|ph| ph.p_type == PT_TLS)?;

    let tdata_start = (base + tls_ph.p_vaddr as usize) as *const u8;
    let tdata_end   = tdata_start.add(tls_ph.p_filesz as usize);
    let tbss_start  = tdata_end;
    let tbss_end    = tdata_start.add(tls_ph.p_memsz as usize);

    Some(TlsInfo { start: tls_ph.p_vaddr, filesz: tls_ph.p_filesz, memsz: tls_ph.p_memsz, align: tls_ph.p_align })
}
