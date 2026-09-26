//! Platform-Level Interrupt Controller (PLIC) driver for RISC-V.
//!
//! [RISC-V PLIC Specification]: https://github.com/riscv/riscv-plic-spec/releases/download/1.0.0/riscv-plic-1.0.0.pdf

use core::mem::offset_of;
use core::num::NonZeroU16;
use core::ptr::NonNull;

use bit_field::BitField;
use memory_addresses::{PhysAddr, VirtAddr};
use volatile::access::{NoAccess, ReadOnly};
use volatile::{VolatileFieldAccess, VolatileRef};

use crate::arch::kernel::interrupts::{EXTERNAL_INTERRUPT_CONTROLLER, ExternalInterruptController};
use crate::arch::mm::paging::{self, PageSize};

const NUMBER_OF_SOURCES: usize = 1024;
const NUMBER_OF_CONTEXTS: usize = 15871;

const INTERRUPT_PENDING_BITS_OFFSET: usize = 0x00_1000;
const INTERRUPT_ENABLE_BITS_OFFSET: usize = 0x00_2000;
const CONTEXT_BASED_REGISTERS: usize = 0x20_0000;

type SourceBitArray = [u32; NUMBER_OF_SOURCES / (u32::BITS as usize)];

#[repr(C, align(4096))]
#[derive(VolatileFieldAccess)]
struct ContextBasedRegisters {
	priority_threshold: u32,
	claim_or_complete: u32,
}

#[repr(C)]
#[derive(VolatileFieldAccess)]
struct PlicControlRegion {
	#[access(NoAccess)]
	_reserved0: u32,
	interrupt_priorities: [u32; NUMBER_OF_SOURCES - 1],
	#[access(ReadOnly)]
	interrupt_pending_bits: SourceBitArray,
	#[access(NoAccess)]
	_reserved3: [u32; (INTERRUPT_ENABLE_BITS_OFFSET - 0x00_1080) / size_of::<u32>()],
	interrupt_enable_bits: [SourceBitArray; NUMBER_OF_CONTEXTS],
	#[access(NoAccess)]
	_reserved2: [u32; (CONTEXT_BASED_REGISTERS - 0x1f_2000) / size_of::<u32>()],
	context_based_registers: [ContextBasedRegisters; NUMBER_OF_CONTEXTS],
}

const _: () =
	assert!(offset_of!(PlicControlRegion, interrupt_pending_bits) == INTERRUPT_PENDING_BITS_OFFSET);
const _: () =
	assert!(offset_of!(PlicControlRegion, interrupt_enable_bits) == INTERRUPT_ENABLE_BITS_OFFSET);
const _: () =
	assert!(offset_of!(PlicControlRegion, context_based_registers) == CONTEXT_BASED_REGISTERS);

pub(crate) struct Plic {
	control_region: VolatileRef<'static, PlicControlRegion>,
	context: u16,
}

impl Plic {
	pub fn set_enable_bit(&mut self, irq_number: u16, value: bool) {
		let source = NonZeroU16::new(irq_number).unwrap();
		let source_idx = usize::from(source.get());
		let plic_ptr = self.control_region.as_mut_ptr();
		unsafe {
			plic_ptr
				.interrupt_enable_bits()
				.map(|slice| {
					slice
						.cast::<SourceBitArray>()
						.offset(isize::try_from(self.context).unwrap())
				})
				.map(|context_slice| {
					context_slice
						.cast::<u32>()
						.offset((source_idx / 32).try_into().unwrap())
				})
				.update(|mut word| {
					word.set_bit(source_idx % 32, value);
					word
				});
		}
	}

	pub fn set_interrupt_priority(&mut self, irq_number: u16, priority: u8) {
		let source = NonZeroU16::new(irq_number).unwrap();
		let plic_ptr = self.control_region.as_mut_ptr();
		unsafe {
			plic_ptr
				.interrupt_priorities()
				.map(|slice| {
					slice
						.cast()
						.offset(isize::try_from(source.get()).unwrap() - 1)
				})
				.write(u32::from(priority));
		}
	}

	pub fn set_priority_threshold(&mut self, threshold: u8) {
		let plic_ptr = self.control_region.as_mut_ptr();
		unsafe {
			plic_ptr
				.context_based_registers()
				.map(|slice| slice.cast().offset(isize::try_from(self.context).unwrap()))
				.priority_threshold()
				.write(u32::from(threshold));
		}
	}

	pub fn claim_interrupt(&mut self) -> Option<NonZeroU16> {
		let plic_ptr = self.control_region.as_mut_ptr();
		unsafe {
			let irq = plic_ptr
				.context_based_registers()
				.map(|slice| slice.cast().offset(isize::try_from(self.context).unwrap()))
				.claim_or_complete()
				.read();
			NonZeroU16::new(irq as u16)
		}
	}

	pub fn complete_interrupt(&mut self, irq_number: u16) {
		let plic_ptr = self.control_region.as_mut_ptr();
		unsafe {
			plic_ptr
				.context_based_registers()
				.map(|slice| slice.cast().offset(isize::try_from(self.context).unwrap()))
				.claim_or_complete()
				.write(u32::from(irq_number));
		}
	}
}

pub fn init_plic(addr: PhysAddr, size: usize, context: u16) {
	assert!(size < usize::try_from(paging::HugePageSize::SIZE).unwrap());
	paging::identity_map::<paging::HugePageSize>(addr);
	let base = VirtAddr::from(addr.as_u64());
	let control_region =
		unsafe { VolatileRef::new(NonNull::new(base.as_mut_ptr::<PlicControlRegion>()).unwrap()) };
	let plic = Plic {
		control_region,
		context,
	};
	*EXTERNAL_INTERRUPT_CONTROLLER.lock() = Some(ExternalInterruptController::Plic(plic));
}
