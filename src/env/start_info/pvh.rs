use core::fmt;
use core::num::NonZero;

use hermit_sync::OnceCell;
use num_enum::TryFromPrimitive;
use pvh::start_info::reader::{IdentityMap, MemMap, StartInfoReader};
use pvh::start_info::{MemmapTableEntry, ModlistEntry};

use super::{MemmapEntry, MemmapType, Module, StartInfo};

static START_INFO: OnceCell<StartInfoReader<'static, IdentityMap>> = OnceCell::new();

pub fn start_info() -> &'static impl StartInfo {
	START_INFO.get().unwrap()
}

pub unsafe fn set_start_info_paddr(start_info_paddr: u32) {
	let start_info = unsafe { StartInfoReader::from_paddr_identity(start_info_paddr).unwrap() };
	START_INFO.set(start_info).unwrap();
}

unsafe impl<M: MemMap> StartInfo for StartInfoReader<'_, M> {
	fn display(&self) -> impl fmt::Display {
		fmt::from_fn(move |f| write!(f, "{self:#?}"))
	}

	fn modules(&self) -> impl Iterator<Item = Module> {
		self.modlist().map(|entry| {
			let entry = entry.raw();
			unsafe { Module::from_pvh(*entry) }
		})
	}

	fn bootargs(&self) -> Option<&str> {
		self.cmdline().map(|cmdline| cmdline.to_str().unwrap())
	}

	fn rsdp_addr(&self) -> Option<NonZero<usize>> {
		let rsdp_addr = usize::try_from(self.raw().rsdp_paddr).unwrap();
		NonZero::new(rsdp_addr)
	}

	fn memmap(&self) -> impl Iterator<Item = MemmapEntry> {
		self.memmap()
			.iter()
			.copied()
			.filter(|memmap_table_entry| memmap_table_entry.ty != 0)
			.map(MemmapEntry::from)
	}
}

impl Module {
	unsafe fn from_pvh(entry: ModlistEntry) -> Self {
		let paddr = usize::try_from(entry.paddr).unwrap();
		let len = usize::try_from(entry.size).unwrap();
		unsafe { Self::new(paddr, len) }
	}
}

impl From<MemmapTableEntry> for MemmapEntry {
	fn from(value: MemmapTableEntry) -> Self {
		let phys_addr = usize::try_from(value.addr).unwrap();
		let len = usize::try_from(value.size).unwrap();
		let ty = u8::try_from(value.ty).unwrap();
		let ty = MemmapType::try_from_primitive(ty).unwrap();

		Self { phys_addr, len, ty }
	}
}
