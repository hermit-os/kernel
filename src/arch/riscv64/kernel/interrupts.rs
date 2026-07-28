use core::mem::offset_of;
use core::num::NonZeroU16;
use core::ptr::NonNull;

use ahash::RandomState;
use bit_field::BitField;
use bitfield_struct::bitfield;
use free_list::PageLayout;
use hashbrown::HashMap;
use hermit_sync::{InterruptTicketMutex, OnceCell, SpinMutex};
use memory_addresses::{PhysAddr, VirtAddr};
use riscv::asm::wfi;
use riscv::interrupt::{Exception, Interrupt, Trap};
use riscv::register::{scause, sie, sip, sstatus, stval};
use trapframe::TrapFrame;
use volatile::access::{NoAccess, ReadOnly, ReadWrite, WriteOnly};
use volatile::{VolatileFieldAccess, VolatileRef};

use crate::arch::mm::paging::{self, BasePageSize, PageSize, PageTableEntryFlags};
use crate::arch::riscv64::kernel::core_local::core_id;
use crate::drivers::InterruptHandlerMap;
use crate::mm::{PageAlloc, PageRangeAllocator};
use crate::scheduler;

pub(crate) enum ExternalInterruptController {
	Plic(Plic),
	Aplic(Aplic),
}

impl ExternalInterruptController {
	fn enable_interrupt(&mut self, irq_number: u16) {
		match self {
			Self::Plic(plic) => plic.set_enable_bit(irq_number, true),
			Self::Aplic(aplic) => aplic.set_enable_bit(irq_number, true),
		}
	}

	#[cfg_attr(
		not(any(feature = "gem-net", feature = "virtio", feature = "pci")),
		allow(dead_code)
	)]
	pub fn set_interrupt_source_mode(&mut self, irq_number: u16, irq_type: u32) {
		match self {
			Self::Plic(_plic) => { /* noop */ }
			Self::Aplic(aplic) => aplic.set_interrupt_source_mode(
				irq_number,
				SourceMode::from(DeviceTreeInterruptType::from(irq_type)),
			),
		}
	}

	fn set_interrupt_priority(&mut self, irq_number: u16, priority: u8) {
		match self {
			Self::Plic(plic) => plic.set_interrupt_priority(irq_number, priority),
			Self::Aplic(aplic) => aplic.set_interrupt_priority(irq_number, priority),
		}
	}

	fn set_priority_threshold(&mut self, threshold: u8) {
		match self {
			Self::Plic(plic) => plic.set_priority_threshold(threshold),
			Self::Aplic(aplic) => aplic.set_priority_threshold(threshold),
		}
	}

	fn claim_interrupt(&mut self) -> Option<NonZeroU16> {
		match self {
			Self::Plic(plic) => plic.claim_interrupt(),
			Self::Aplic(aplic) => aplic.claim_interrupt(),
		}
	}

	fn complete_interrupt(&mut self, irq_number: u16) {
		match self {
			Self::Plic(plic) => plic.complete_interrupt(irq_number),
			Self::Aplic(aplic) => aplic.complete_interrupt(irq_number),
		}
	}
}

// Reference: The RISC-V Advanced Interrupt Architecture, Version 1.0, Chapter 4.5.1
#[bitfield(u32)]
struct DomainConfig {
	// BE - Big Endian (0 = little endian, 1 = big endian)
	#[bits(1)]
	big_endian: bool,

	#[bits(1)]
	__: bool,

	// DM - Domain Mode (0 = direct delivery, 1 = msi delivery)
	#[bits(1)]
	msi_delivery: bool,

	#[bits(5)]
	__: u8,

	// IE - Interrupt Enable (0 = disabled, 1 = enabled)
	#[bits(1)]
	interrupt_enable: bool,

	#[bits(23)]
	__: u32,
}

// Reference: The RISC-V Advanced Interrupt Architecture, Version 1.0, Chapter 4.5.2
#[bitfield(u32)]
struct SourceConfigRegister {
	#[bits(10)]
	__payload: u16,

	#[bits(1)]
	delegated: u8,

	#[bits(21)]
	__: u32,
}

#[repr(u32)]
enum DeviceTreeInterruptType {
	/// Low to high edge sensitive type enabled
	LowToHighEdge = 1,
	/// Active low level sensitive type enabled
	ActiveLowLevel = 2,
	/// Active high level sensitive type enabled
	ActiveHighLevel = 4,
	/// High to low edge sensitive type enabled
	HighToLowEdge = 8,
}
impl From<u32> for DeviceTreeInterruptType {
	fn from(value: u32) -> Self {
		match value {
			1 => Self::LowToHighEdge,
			2 => Self::ActiveLowLevel,
			4 => Self::ActiveHighLevel,
			8 => Self::HighToLowEdge,
			_ => panic!("invalid DeviceTreeInterruptType bits"),
		}
	}
}

// Reference: The RISC-V Advanced Interrupt Architecture, Version 1.0, Chapter 4.5.2
#[derive(Debug)]
#[repr(u8)]
enum SourceMode {
	// Inactive in this domain (and not delegated)
	Inactive = 0,
	// Active, detached from the source wire
	Detached = 1,
	// Edge-sensitive; interrupt asserted on rising edge
	Edge1 = 4,
	// Edge-sensitive; interrupt asserted on falling edge
	Edge0 = 5,
	// Level-sensitive; interrupt asserted when high
	Level1 = 6,
	// Level-sensitive; interrupt asserted when low
	Level0 = 7,
}
impl SourceMode {
	const fn into_bits(self) -> u8 {
		self as _
	}

	const fn from_bits(bits: u8) -> Self {
		match bits {
			0 => Self::Inactive,
			1 => Self::Detached,
			4 => Self::Edge1,
			5 => Self::Edge0,
			6 => Self::Level1,
			7 => Self::Level0,
			_ => panic!("invalid SourceMode bits"),
		}
	}
}
impl From<DeviceTreeInterruptType> for SourceMode {
	fn from(value: DeviceTreeInterruptType) -> Self {
		match value {
			DeviceTreeInterruptType::LowToHighEdge => Self::Edge1,
			DeviceTreeInterruptType::ActiveLowLevel => Self::Level0,
			DeviceTreeInterruptType::ActiveHighLevel => Self::Level1,
			DeviceTreeInterruptType::HighToLowEdge => Self::Edge0,
		}
	}
}

// Reference: The RISC-V Advanced Interrupt Architecture, Version 1.0, Chapter 4.5.2
#[bitfield(u32)]
struct SourceConfigSourceMode {
	#[bits(3)]
	mode: SourceMode,

	#[bits(7)]
	__: u8,

	#[bits(1)]
	delegated: u8,

	#[bits(21)]
	__: u32,
}

// Reference: The RISC-V Advanced Interrupt Architecture, Version 1.0, Chapter 4.5.2
#[bitfield(u32)]
struct SourceConfigDelegated {
	#[bits(10)]
	child_index: u16,

	#[bits(1)]
	delegated: u8,

	#[bits(21)]
	__: u32,
}

// Reference: The RISC-V Advanced Interrupt Architecture, Version 1.0, Chapter 4.5.16
#[bitfield(u32)]
struct TargetDirectDelivery {
	// Priority number for interrupt source
	//
	// Lower values indicate higher priority. The maximum priority is 0, and the minimum
	// priority is 255.
	#[bits(8)]
	priority: u8,

	#[bits(10)]
	__: u16,

	// Hart to which the interrupt for this source will be delivered
	#[bits(14)]
	hart_index: u16,
}

// Reference: The RISC-V Advanced Interrupt Architecture, Version 1.0, Chapter 4.5.16
#[bitfield(u32)]
struct TargetMsiDelivery {
	// External interrupt identity
	#[bits(11)]
	eiid: u16,

	#[bits(1)]
	__: u8,

	// Number of the target hart’s guest interrupt file to which MSIs will be sent.
	// Only relevant if domain’s harts implement hypervisor extension.
	#[bits(6)]
	__guest_index: u8,

	// Hart to which the interrupt for this source will be forwarded
	#[bits(14)]
	hart_index: u16,
}

// Reference: The RISC-V Advanced Interrupt Architecture, Version 1.0, Chapter 4.5.16
#[bitfield(u32)]
struct TargetRegister {
	#[bits(18)]
	__payload: u32,

	// Target hart index
	#[bits(14)]
	hart_index: u16,
}

// Reference: The RISC-V Advanced Interrupt Architecture, Version 1.0, Table 4.1
#[repr(C)]
#[derive(VolatileFieldAccess)]
struct AplicControlRegion {
	// Domain configuration
	// Bit 8: IE - Interrupt Enable
	// Bit 2: DM - Domain Mode (0 = direct delivery, 1 = msi delivery)
	// Bit 0: BE - Big Endian
	#[access(ReadWrite)]
	domaincfg: DomainConfig,
	#[access(ReadWrite)]
	sourcecfg: [SourceConfigRegister; NUMBER_OF_SOURCES - 1],
	#[access(NoAccess)]
	_reserved_sourcecfg: [u32; (0x1bc0 - 0x1000) / size_of::<SourceConfigRegister>()],

	// MSI address configuration registers (m-mode only)
	#[access(NoAccess)]
	_mmsiaddrcfg: u32,
	#[access(NoAccess)]
	_mmsiaddrcfgh: u32,
	#[access(NoAccess)]
	_smsiaddrcfg: u32,
	#[access(NoAccess)]
	_smsiaddrcfgh: u32,
	#[access(NoAccess)]
	_reserved_msiaddrcfg: [u32; (0x1c00 - 0x1bd0) / size_of::<u32>()],

	// Set interrupt pending bits
	#[access(ReadWrite)]
	_setip: [u32; 32],
	#[access(NoAccess)]
	_reserved_setip: [u32; (0x1cdc - 0x1c80) / size_of::<u32>()],
	// Set interrupt pending bit number
	#[access(WriteOnly)]
	_setipnum: u32,
	#[access(NoAccess)]
	_reserved_setipnum: [u32; (0x1d00 - 0x1ce0) / size_of::<u32>()],

	// Clear interrupt pending bits
	#[access(ReadWrite)]
	_in_clrip: [u32; 32],
	#[access(NoAccess)]
	_reserved_in_clrip: [u32; (0x1ddc - 0x1d80) / size_of::<u32>()],
	// Clear interrupt pending bit number
	#[access(WriteOnly)]
	_clripnum: u32,
	#[access(NoAccess)]
	_reserved_clripnum: [u32; (0x1e00 - 0x1de0) / size_of::<u32>()],

	// Set interrupt enable bits
	#[access(ReadWrite)]
	_setie: [u32; 32],
	#[access(NoAccess)]
	_reserved_setie: [u32; (0x1edc - 0x1e80) / size_of::<u32>()],
	// Set interrupt enable bit number
	#[access(WriteOnly)]
	setienum: u32,
	#[access(NoAccess)]
	_reserved_setienum: [u32; (0x1f00 - 0x1ee0) / size_of::<u32>()],

	// Clear interrupt enable bits
	#[access(WriteOnly)]
	_clrie: [u32; 32],
	#[access(NoAccess)]
	_reserved_clrie: [u32; (0x1fdc - 0x1f80) / size_of::<u32>()],
	// Clear interrupt enable bit number
	#[access(WriteOnly)]
	clrienum: u32,
	#[access(NoAccess)]
	_reserved_clrienum: [u32; (0x2000 - 0x1fe0) / size_of::<u32>()],

	// Set interrupt pending bit by number (little-endian)
	#[access(WriteOnly)]
	_setipnum_le: u32,
	// Set interrupt pending bit by number (big-endian)
	#[access(WriteOnly)]
	_setipnum_be: u32,
	#[access(NoAccess)]
	_reserved_setipnum_be: [u32; (0x3000 - 0x2008) / size_of::<u32>()],

	// Generate MSI
	#[access(ReadWrite)]
	_genmsi: u32,

	// Interrupt Targets
	target: [TargetRegister; NUMBER_OF_SOURCES - 1],
}
const _: () = assert!(offset_of!(AplicControlRegion, _mmsiaddrcfg) == 0x1bc0);
const _: () = assert!(offset_of!(AplicControlRegion, _smsiaddrcfg) == 0x1bc8);
const _: () = assert!(offset_of!(AplicControlRegion, _setip) == 0x1c00);
const _: () = assert!(offset_of!(AplicControlRegion, _setipnum) == 0x1cdc);
const _: () = assert!(offset_of!(AplicControlRegion, _in_clrip) == 0x1d00);
const _: () = assert!(offset_of!(AplicControlRegion, _clripnum) == 0x1ddc);
const _: () = assert!(offset_of!(AplicControlRegion, _setie) == 0x1e00);
const _: () = assert!(offset_of!(AplicControlRegion, setienum) == 0x1edc);
const _: () = assert!(offset_of!(AplicControlRegion, _clrie) == 0x1f00);
const _: () = assert!(offset_of!(AplicControlRegion, clrienum) == 0x1fdc);
const _: () = assert!(offset_of!(AplicControlRegion, _setipnum_le) == 0x2000);
const _: () = assert!(offset_of!(AplicControlRegion, _genmsi) == 0x3000);
const _: () = assert!(size_of::<AplicControlRegion>() == 0x4000);

// Reference: The RISC-V Advanced Interrupt Architecture, Version 1.0, Chapter 4.8.1.4
#[bitfield(u32)]
struct TopInterrupt {
	#[bits(8)]
	priority: u8,

	#[bits(8)]
	__: u8,

	#[bits(10)]
	identity: u16,

	#[bits(6)]
	__: u8,
}

// Reference: The RISC-V Advanced Interrupt Architecture, Version 1.0, Chapter 4.8.1
#[repr(C)]
#[derive(VolatileFieldAccess)]
struct InterruptDeliveryControl {
	// Interrupt delivery enable (0 = disabled, 1 = enabled)
	#[access(ReadWrite)]
	idelivery: u32,

	// Interrupt force (0 = no effect, 1 = force interrupt)
	// For testing only.
	#[access(ReadWrite)]
	iforce: u32,

	// Interrupt enable threshold (0 = all interrupts enabled, n = interrupts with priority > n enabled)
	#[access(ReadWrite)]
	ithreshold: u32,

	#[access(NoAccess)]
	_reserved: [u32; 3],

	// Top interrupt
	#[access(ReadOnly)]
	topi: TopInterrupt,

	// Claim top interrupt
	#[access(ReadOnly)]
	claimi: TopInterrupt,
}
const _: () = assert!(offset_of!(InterruptDeliveryControl, topi) == 0x18);
const _: () = assert!(offset_of!(InterruptDeliveryControl, claimi) == 0x1c);
const _: () = assert!(size_of::<InterruptDeliveryControl>() == 0x20);

const APLIC_DIRECT_DELIVERY_MODE_MAX_HARTS: u16 = 512;

// In direct delivery mode the number of harts is limited to 512.
type InterruptDeliveryControlArray =
	[InterruptDeliveryControl; APLIC_DIRECT_DELIVERY_MODE_MAX_HARTS as usize];

pub(crate) struct Aplic {
	control_region: VolatileRef<'static, AplicControlRegion>,
	interrupt_delivery_control: VolatileRef<'static, InterruptDeliveryControlArray>,
	ipriolen: u8,
}

impl Aplic {
	fn init(&mut self) {
		let aplic_ptr = self.control_region.as_mut_ptr();
		let mut domaincfg = aplic_ptr.domaincfg().read();
		if domaincfg.big_endian() {
			domaincfg.set_big_endian(false);
			aplic_ptr.domaincfg().write(domaincfg);
			assert!(
				!aplic_ptr.domaincfg().read().big_endian(),
				"Only little-endian is supported"
			);
		}

		let aplic_ptr = self.control_region.as_mut_ptr();
		let mut domaincfg = aplic_ptr.domaincfg().read();
		if domaincfg.msi_delivery() {
			domaincfg.set_msi_delivery(false);
			aplic_ptr.domaincfg().write(domaincfg);
			assert!(
				!aplic_ptr.domaincfg().read().msi_delivery(),
				"APLIC does not support direct delivery."
			);
		}

		self.ipriolen = self.probe_ipriolen();

		let aplic_ptr = self.control_region.as_mut_ptr();
		aplic_ptr.domaincfg().update(|mut cfg| {
			cfg.set_interrupt_enable(true);
			cfg
		});

		let hart_idc = unsafe {
			self.interrupt_delivery_control
				.as_mut_ptr()
				.map(|control| control.cast().offset(Aplic::get_hart_index() as isize))
		};
		hart_idc.idelivery().write(1);
	}

	/// Determines the number of implemented interrupt priority bits (IPRIOLEN)
	/// by probing the first source's target WARL register.
	///
	// Reference: The RISC-V Advanced Interrupt Architecture, Version 1.0, Chapter 4.5.16
	fn probe_ipriolen(&mut self) -> u8 {
		let saved_source_mode = self.get_interrupt_source_mode(1);
		self.set_interrupt_source_mode(1, SourceMode::Detached);
		let target = unsafe {
			self.control_region
				.as_mut_ptr()
				.target()
				.map(|slice| slice.cast::<TargetRegister>())
		};
		let saved_target_reg = target.read();
		target.write(TargetRegister::from(
			TargetDirectDelivery::new()
				.with_hart_index(Aplic::get_hart_index())
				.with_priority(0xff)
				.into_bits(),
		));
		let readback = TargetDirectDelivery::from(target.read().into_bits()).priority();
		target.write(saved_target_reg);
		self.set_interrupt_source_mode(1, saved_source_mode);

		let ipriolen = (u8::BITS - readback.leading_zeros()) as u8;
		assert!(
			(1..=8).contains(&ipriolen),
			"APLIC IPRIOLEN must be between 1 and 8, but probed {ipriolen}"
		);
		ipriolen
	}

	fn get_hart_index() -> u16 {
		// The core identifier and the hart index of an core in an interrupt domain are not necessarily the same.
		// The hart index can be extracted from the devicetree as following.
		// 1. Find all core nodes cpu@X
		// 2. For each core find core local interrupter node (<compatible> = "riscv,cpu-intc") and get its phandle
		// 3. Find aplic node (<compatible> = "riscv,aplic", <status> != "disabled") for active interrupt domain
		// 4. Property <interrupts-extended> is a list of tuples with the following format: <phandle cpu-intc> <interrupt-specifier> ...
		//    For supervisor external interrupts interrupt-specifier = 0x9
		//    For machine external interrupts interrupt-specifier = 0xb
		// 5. The index of the tuple in the list is the hart index of the core in the interrupt domain.
		//
		// The core identifier is identical to the interrupt domain hart index if
		// - the core identifier are continous and start with 0 and
		// - the interrupt-extended property of the aplic node is ordered by core identifier.
		// On QEMU virt machine these assumptions hold true.

		let core_id: u16 = core_id().try_into().unwrap();
		assert!(
			core_id < APLIC_DIRECT_DELIVERY_MODE_MAX_HARTS,
			"APLIC direct delivery mode supports only {APLIC_DIRECT_DELIVERY_MODE_MAX_HARTS} harts, but core_id is {core_id}"
		);
		core_id
	}

	fn set_enable_bit(&mut self, irq_number: u16, value: bool) {
		if value {
			self.control_region
				.as_mut_ptr()
				.setienum()
				.write(u32::from(irq_number));
		} else {
			self.control_region
				.as_mut_ptr()
				.clrienum()
				.write(u32::from(irq_number));
		}
	}

	fn set_interrupt_source_mode(&mut self, irq_number: u16, mode: SourceMode) {
		let sourcecfg = unsafe {
			self.control_region.as_mut_ptr().sourcecfg().map(|slice| {
				slice
					.cast()
					.offset(isize::try_from(irq_number).unwrap() - 1)
			})
		};
		sourcecfg.write(SourceConfigRegister::from(
			SourceConfigSourceMode::new().with_mode(mode).into_bits(),
		));
	}

	fn get_interrupt_source_mode(&mut self, irq_number: u16) -> SourceMode {
		let sourcecfg = unsafe {
			self.control_region.as_mut_ptr().sourcecfg().map(|slice| {
				slice
					.cast()
					.offset(isize::try_from(irq_number).unwrap() - 1)
			})
		};
		let sourcecfg_value: u32 = sourcecfg.read();
		SourceConfigSourceMode::from(sourcecfg_value).mode()
	}

	fn set_interrupt_priority(&mut self, irq_number: u16, priority: u8) {
		// Clamp to the largest representable priority value (lowest priority).
		let max_priority = ((1u16 << self.ipriolen) - 1) as u8;
		let priority = priority.min(max_priority);

		let target = unsafe {
			self.control_region.as_mut_ptr().target().map(|slice| {
				slice
					.cast()
					.offset(isize::try_from(irq_number).unwrap() - 1)
			})
		};
		let new_value = TargetRegister::from(
			TargetDirectDelivery::new()
				.with_hart_index(Aplic::get_hart_index())
				.with_priority(priority)
				.into_bits(),
		);
		target.write(new_value);
	}

	fn set_priority_threshold(&mut self, threshold: u8) {
		let hart_idc = unsafe {
			self.interrupt_delivery_control
				.as_mut_ptr()
				.map(|control| control.cast().offset(Aplic::get_hart_index() as isize))
		};
		hart_idc.ithreshold().write(u32::from(threshold));
	}

	fn claim_interrupt(&mut self) -> Option<NonZeroU16> {
		let hart_idc = unsafe {
			self.interrupt_delivery_control
				.as_mut_ptr()
				.map(|control| control.cast().offset(Aplic::get_hart_index() as isize))
		};
		let claimi = hart_idc.claimi().read();
		NonZeroU16::new(claimi.identity())
	}

	fn complete_interrupt(&mut self, _irq_number: u16) {
		// reading claimi register automatically completes the interrupt in direct delivery mode.
	}
}

pub(crate) fn init_aplic(addr: PhysAddr, size: usize) {
	assert!(
		size == 32 * 1024,
		"Expected 32 KiB control region for APLIC in direct delivery mode"
	);

	let layout = PageLayout::from_size(size).unwrap();
	let page_range = PageAlloc::allocate(layout).unwrap();
	let control_region_addr = VirtAddr::from(page_range.start());

	let mut flags = PageTableEntryFlags::empty();
	flags.device().normal().writable().execute_disable();
	paging::map::<BasePageSize>(
		control_region_addr,
		addr,
		size / usize::try_from(BasePageSize::SIZE).unwrap(),
		flags,
	);

	let control_region = unsafe {
		VolatileRef::new(
			NonNull::new(control_region_addr.as_mut_ptr::<AplicControlRegion>()).unwrap(),
		)
	};
	let interrupt_delivery_control = unsafe {
		VolatileRef::new(
			NonNull::new(
				VirtAddr::from(control_region_addr.as_u64() + 0x4000)
					.as_mut_ptr::<InterruptDeliveryControlArray>(),
			)
			.unwrap(),
		)
	};
	let mut aplic = Aplic {
		control_region,
		interrupt_delivery_control,
		ipriolen: 0,
	};
	aplic.init();

	*EXTERNAL_INTERRUPT_CONTROLLER.lock() = Some(ExternalInterruptController::Aplic(aplic));
}

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
	fn set_enable_bit(&mut self, irq_number: u16, value: bool) {
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

	fn set_interrupt_priority(&mut self, irq_number: u16, priority: u8) {
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

	fn set_priority_threshold(&mut self, threshold: u8) {
		let plic_ptr = self.control_region.as_mut_ptr();
		unsafe {
			plic_ptr
				.context_based_registers()
				.map(|slice| slice.cast().offset(isize::try_from(self.context).unwrap()))
				.priority_threshold()
				.write(u32::from(threshold));
		}
	}

	fn claim_interrupt(&mut self) -> Option<NonZeroU16> {
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

	fn complete_interrupt(&mut self, irq_number: u16) {
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

pub(crate) static EXTERNAL_INTERRUPT_CONTROLLER: SpinMutex<Option<ExternalInterruptController>> =
	SpinMutex::new(None);

static INTERRUPT_HANDLERS: OnceCell<InterruptHandlerMap> = OnceCell::new();

/// Init Interrupts
pub(crate) fn install() {
	unsafe {
		// Install trap handler
		trapframe::init();
		// Enable external interrupts
		sie::set_sext();
	}
}

/// Init PLIC
pub(crate) fn init_plic(addr: PhysAddr, size: usize, context: u16) {
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

/// Enable Interrupts
#[inline]
pub(crate) fn enable() {
	unsafe {
		sstatus::set_sie();
	}
}

static IRQ_NAMES: InterruptTicketMutex<HashMap<u8, &'static str, RandomState>> =
	InterruptTicketMutex::new(HashMap::with_hasher(RandomState::with_seeds(0, 0, 0, 0)));

#[allow(dead_code)]
pub(crate) fn add_irq_name(irq_number: u8, name: &'static str) {
	debug!("Register name \"{name}\" for interrupt {irq_number}");
	IRQ_NAMES.lock().insert(irq_number, name);
}

/// Waits for the next interrupt (Only Supervisor-level software/timer interrupt for now)
/// and calls the specific handler
#[inline]
pub(crate) fn enable_and_wait() {
	unsafe {
		//Enable Supervisor-level software interrupts
		sie::set_ssoft();
		//sie::set_sext();
		debug!("Wait {:x?}", sie::read());
		loop {
			wfi();
			// Interrupts are disabled at this point, so a pending interrupt will
			// resume the execution. We still have to check if a interrupt is pending
			// because the WFI instruction could be implemented as NOP (The RISC-V Instruction Set ManualVolume II: Privileged Architecture)

			let pending_interrupts = sip::read();

			// trace!("sip: {:x?}", pending_interrupts);
			#[cfg(feature = "smp")]
			if pending_interrupts.ssoft() {
				//Clear Supervisor-level software interrupt
				core::arch::asm!(
					"csrc sip, {ssoft_mask}",
					ssoft_mask = in(reg) 0x2,
				);
				trace!("SOFT");
				//Disable Supervisor-level software interrupt
				sie::clear_ssoft();
				crate::arch::kernel::scheduler::wakeup_handler();
				break;
			}

			if pending_interrupts.sext() {
				trace!("EXT");
				external_handler();
				break;
			}

			if pending_interrupts.stimer() {
				// // Disable Supervisor-level software interrupt, wakeup not needed
				// sie::clear_ssoft();

				debug!("sip: {pending_interrupts:x?}");
				trace!("TIMER");
				crate::arch::kernel::scheduler::timer_handler();
				break;
			}
		}
	}
}

/// Disable Interrupts
#[inline]
pub(crate) fn disable() {
	unsafe { sstatus::clear_sie() };
}

/// Currently not needed because we use the trapframe crate
pub(crate) fn install_handlers(handlers: InterruptHandlerMap) {
	let mut ctrl_guard = EXTERNAL_INTERRUPT_CONTROLLER.lock();
	let ctrl = ctrl_guard.as_mut().unwrap();

	for irq_number in handlers.keys() {
		// Set priority to 255 (lowest priority)
		ctrl.set_interrupt_priority(u16::from(*irq_number), u8::MAX);
		ctrl.enable_interrupt(u16::from(*irq_number));
	}
	ctrl.set_priority_threshold(0);

	INTERRUPT_HANDLERS.set(handlers).unwrap();
}

// Derived from rCore: https://github.com/rcore-os/rCore
/// Dispatch and handle interrupt.
///
/// This function is called from `trap.S` which is in the trapframe crate.
#[unsafe(no_mangle)]
pub extern "C" fn trap_handler(tf: &mut TrapFrame) {
	let scause = scause::read();
	let cause = scause.cause();
	let cause = Trap::<Interrupt, Exception>::try_from(cause).unwrap();
	let stval = stval::read();
	let sepc = tf.sepc;
	trace!("Interrupt: {cause:?}");
	trace!("tf = {tf:x?} ");
	trace!("stval = {stval:x}");
	trace!("sepc = {sepc:x}");
	trace!("SSTATUS FS = {:?}", sstatus::read().fs());

	match cause {
		Trap::Interrupt(Interrupt::SupervisorExternal) => external_handler(),
		#[cfg(feature = "smp")]
		Trap::Interrupt(Interrupt::SupervisorSoft) => {
			crate::arch::kernel::scheduler::wakeup_handler();
		}
		Trap::Interrupt(Interrupt::SupervisorTimer) => {
			crate::arch::kernel::scheduler::timer_handler();
		}
		cause => {
			error!("Interrupt: {cause:?}");
			error!("tf = {tf:x?} ");
			error!("stval = {stval:x}");
			error!("sepc = {sepc:x}");
			error!("SSTATUS FS = {:?}", sstatus::read().fs());
			scheduler::abort();
		}
	}
	trace!("Interrupt end");
}

/// Handles external interrupts
fn external_handler() {
	use crate::arch::kernel::core_local::core_scheduler;
	use crate::scheduler::PerCoreSchedulerExt;

	// Claim interrupt
	let mut ctrl_guard = EXTERNAL_INTERRUPT_CONTROLLER.lock();
	let ctrl = ctrl_guard.as_mut().unwrap();
	if let Some(irq) = ctrl.claim_interrupt() {
		let irq = irq.get();
		debug!("External INT: {irq}");

		if let Some(handlers) = INTERRUPT_HANDLERS.get()
			&& let Ok(irq) = u8::try_from(irq)
			&& let Some(queue) = handlers.get(&irq)
		{
			for handler in queue.iter() {
				handler();
			}
		}
		crate::executor::run();

		core_scheduler().reschedule();

		ctrl.complete_interrupt(irq);
	}
}

pub(crate) fn print_statistics() {}
