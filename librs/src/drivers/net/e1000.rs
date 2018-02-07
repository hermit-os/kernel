// Copyright (c) 2017 Colin Finck, RWTH Aachen University
//
// MIT License
//
// Permission is hereby granted, free of charge, to any person obtaining
// a copy of this software and associated documentation files (the
// "Software"), to deal in the Software without restriction, including
// without limitation the rights to use, copy, modify, merge, publish,
// distribute, sublicense, and/or sell copies of the Software, and to
// permit persons to whom the Software is furnished to do so, subject to
// the following conditions:
//
// The above copyright notice and this permission notice shall be
// included in all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
// EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
// MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
// NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE
// LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
// OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION
// WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.

use alloc::boxed::Box;
use arch::apic;
use arch::idt;
use arch::irq;
use arch::mm::paging;
use arch::mm::paging::{BasePageSize, PageSize, PageTableEntryFlags};
use arch::mm::virtualmem;
use arch::pci;
use arch::pic;
use arch::processor;
use core::{mem, ptr, u32};
use drivers::net::*;
use drivers::net::lwip::*;
use mm;


const E1000_CTRL_REGISTER:   usize = 0x0_0000;
const E1000_CTRL_LINK_RESET:                  u32 = 1 << 3;
const E1000_CTRL_AUTO_SPEED_DETECTION_ENABLE: u32 = 1 << 5;
const E1000_CTRL_SET_LINK_UP:                 u32 = 1 << 6;
const E1000_CTRL_INVERT_LOSS_OF_SIGNAL:       u32 = 1 << 7;
const E1000_CTRL_FORCE_SPEED:                 u32 = 1 << 11;
const E1000_CTRL_RESET:                       u32 = 1 << 26;
const E1000_CTRL_VLAN_MODE_ENABLE:            u32 = 1 << 30;
const E1000_CTRL_PHY_RESET:                   u32 = 1 << 31;

const E1000_STATUS_REGISTER: usize = 0x0_0008;
const E1000_STATUS_FULL_DUPLEX: u32 = 1 << 0;
const E1000_STATUS_SPEED_100:   u32 = 1 << 6;
const E1000_STATUS_SPEED_1000:  u32 = 1 << 7;

const E1000_EERD_REGISTER: usize = 0x0_0014;
const E1000_EERD_START:      u32 = 1 << 0;
const E1000_EERD_DONE:       u32 = 1 << 4;
const E1000_EERD_ADDR_SHIFT: usize = 8;
const E1000_EERD_DATA_SHIFT: usize = 16;

const E1000_ICR_REGISTER:  usize = 0x0_00C0;

const E1000_IMS_REGISTER:  usize = 0x0_00D0;
const E1000_IMS_RECEIVE_SEQUENCE_ERROR:                   u32 = 1 << 3;
const E1000_IMS_RECEIVE_DESCRIPTOR_MINIMUM_THRESHOLD_HIT: u32 = 1 << 4;
const E1000_IMS_RECEIVER_FIFO_OVERRUN:                    u32 = 1 << 6;
const E1000_IMS_RECEIVER_TIMER_INTERRUPT:                 u32 = 1 << 7;

const E1000_IMC_REGISTER:  usize = 0x0_00D8;

const E1000_RCTL_REGISTER: usize = 0x0_0100;
const E1000_RCTL_ENABLE:             u32 = 1 << 1;
const E1000_RCTL_BROADCAST_ACCEPT:   u32 = 1 << 15;
const E1000_RCTL_STRIP_ETHERNET_CRC: u32 = 1 << 26;

/// Refer to Intel 8254x Manual, Table 13-76. TCTL Register Bit Description.
const E1000_TCTL_REGISTER: usize = 0x0_0400;
const E1000_TCTL_ENABLE:                         u32 = 1 << 1;
const E1000_TCTL_PAD_SHORT_PACKET:               u32 = 1 << 3;
const E1000_TCTL_COLLISION_THRESHOLD_DEFAULT:    u32 = 16;
const E1000_TCTL_COLLISION_THRESHOLD_SHIFT:      usize = 4;
const E1000_TCTL_COLLISION_DISTANCE_FULL_DUPLEX: u32 = 64;
const E1000_TCTL_COLLISION_DISTANCE_HALF_DUPLEX: u32 = 512;
const E1000_TCTL_COLLISION_DISTANCE_SHIFT:       usize = 12;

/// Refer to Intel 8254x Manual, Table 13-77. TIPG Register Bit Description.
const E1000_TIPG_REGISTER: usize = 0x0_0410;
const E1000_TIPG_TRANSMIT_TIME_DEFAULT: u32 = 10;
const E1000_TIPG_RECEIVE_TIME1_DEFAULT: u32 = 8;
const E1000_TIPG_RECEIVE_TIME1_SHIFT:   usize = 10;
const E1000_TIPG_RECEIVE_TIME2_DEFAULT: u32 = 6;
const E1000_TIPG_RECEIVE_TIME2_SHIFT:   usize = 20;

const E1000_RDBAL_REGISTER:  usize = 0x0_2800;
const E1000_RDBAH_REGISTER:  usize = 0x0_2804;
const E1000_RDLEN_REGISTER:  usize = 0x0_2808;
const E1000_RDH_REGISTER:    usize = 0x0_2810;
const E1000_RDT_REGISTER:    usize = 0x0_2818;

const E1000_TDBAL_REGISTER:  usize = 0x0_3800;
const E1000_TDBAH_REGISTER:  usize = 0x0_3804;
const E1000_TDLEN_REGISTER:  usize = 0x0_3808;
const E1000_TDH_REGISTER:    usize = 0x0_3810;
const E1000_TDT_REGISTER:    usize = 0x0_3818;

const E1000_MTA_REGISTER:    usize = 0x0_5200;

/// Refer to Intel 8254x Manual, Tables 13-90. RAL Register Bit Description and 13-91. RAH Register Bit Description.
const E1000_RAL_REGISTER:    usize = 0x0_5400;
const E1000_RAH_REGISTER:    usize = 0x0_5404;
const E1000_RA_ADDRESS_VALID: u64 = (1 << 31) << 32;

const E1000_RX_DESCRIPTOR_STATUS_DONE: u8 = 1 << 0;
const E1000_RX_DESCRIPTOR_STATUS_EOP:  u8 = 1 << 1;

const E1000_TX_DESCRIPTOR_STATUS_DONE:       u8 = 1 << 0;
const E1000_TX_DESCRIPTOR_CMD_EOP:           u8 = 1 << 0;
const E1000_TX_DESCRIPTOR_CMD_INSERT_FCS:    u8 = 1 << 1;
const E1000_TX_DESCRIPTOR_CMD_REPORT_STATUS: u8 = 1 << 3;

const NUM_RX_DESCRIPTORS: usize = 64;
const NUM_TX_DESCRIPTORS: usize = 64;
const RX_BUFFER_SIZE:     usize = 2048;
const TX_BUFFER_SIZE:     usize = 1792;

const INTERRUPT_MASK: u32 = E1000_IMS_RECEIVE_SEQUENCE_ERROR |
	E1000_IMS_RECEIVE_DESCRIPTOR_MINIMUM_THRESHOLD_HIT |
	E1000_IMS_RECEIVER_FIFO_OVERRUN |
	E1000_IMS_RECEIVER_TIMER_INTERRUPT;

#[repr(C, packed)]
struct RxDescriptor {
	pub addr: u64,
	pub length: u16,
	pub checksum: u16,
	pub status: u8,
	pub error: u8,
	pub special: u16,
}

#[repr(C, packed)]
struct TxDescriptor {
	pub addr: u64,
	pub length: u16,
	pub cso: u8,
	pub cmd: u8,
	pub status: u8,
	pub css: u8,
	pub special: u16,
}

type RxBufferType = [[u8; RX_BUFFER_SIZE]; NUM_RX_DESCRIPTORS];
type RxDescriptorType = [RxDescriptor; NUM_RX_DESCRIPTORS];
type TxBufferType = [[u8; TX_BUFFER_SIZE]; NUM_TX_DESCRIPTORS];
type TxDescriptorType = [TxDescriptor; NUM_TX_DESCRIPTORS];

static SUPPORTED_DEVICES: &[(u16, u16)] = &[
	(0x8086, 0x1000),	// Intel E1000 (82542)
	(0x8086, 0x1001),	// Intel E1000 (82543GC FIBER)
	(0x8086, 0x1004),	// Intel E1000 (82543GC COPPER)
	(0x8086, 0x1008),	// Intel E1000 (82544EI COPPER)
	(0x8086, 0x1009),	// Intel E1000 (82544EI FIBER)
	(0x8086, 0x100C),	// Intel E1000 (82544GC COPPER)
	(0x8086, 0x100D),	// Intel E1000 (82544GC LOM)
	(0x8086, 0x100E),	// Intel E1000 (82540EM)
	(0x8086, 0x100F),	// Intel E1000 (82545EM COPPER)
	(0x8086, 0x1010),	// Intel E1000 (82546EB COPPER)
	(0x8086, 0x1011),	// Intel E1000 (82545EM FIBER)
	(0x8086, 0x1012),	// Intel E1000 (82546EB FIBER)
	(0x8086, 0x1015),	// Intel E1000 (82540EM LOM)
	(0x8086, 0x1016),	// Intel E1000 (82540EP LOM)
	(0x8086, 0x1017),	// Intel E1000 (82540EP)
	(0x8086, 0x101D),	// Intel E1000 (82546EB QUAD COPPER)
	(0x8086, 0x101E),	// Intel E1000 (82540EP LP)
	(0x8086, 0x1026),	// Intel E1000 (82545GM COPPER)
	(0x8086, 0x1027),	// Intel E1000 (82545GM FIBER)
	(0x8086, 0x1028),	// Intel E1000 (82545GM SERDES)
	(0x8086, 0x1075),	// Intel E1000 (82547GI)
	(0x8086, 0x1076),	// Intel E1000 (82541GI)
	(0x8086, 0x1077),	// Intel E1000 (82541GI MOBILE)
	(0x8086, 0x1079),	// Intel E1000 (82546GB COPPER)
	(0x8086, 0x107A),	// Intel E1000 (82546GB FIBER)
	(0x8086, 0x107B),	// Intel E1000 (82546GB SERDES)
	(0x8086, 0x107C),	// Intel E1000 (82541GI LF)
	(0x8086, 0x108A),	// Intel E1000 (82546GB PCIE)
	(0x8086, 0x1099),	// Intel E1000 (82546GB QUAD COPPER)
	(0x8086, 0x10B5),	// Intel E1000 (82546GB QUAD COPPER KSP3)
];

static mut ADAPTER: Option<Box<E1000NetworkAdapter>> = None;


pub struct E1000NetworkAdapter {
	base_address: usize,
	pci_adapter: pci::PciAdapter,
	rx_buffers: *mut RxBufferType,
	rx_descriptors: *mut RxDescriptorType,
	rx_tail: usize,
	tx_buffers: *mut TxBufferType,
	tx_descriptors: *mut TxDescriptorType,
	tx_tail: usize,
}

impl E1000NetworkAdapter {
	const fn new(pci_adapter: pci::PciAdapter) -> Self {
		Self {
			base_address: 0,
			pci_adapter: pci_adapter,
			rx_buffers: 0 as *mut RxBufferType,
			rx_descriptors: 0 as *mut RxDescriptorType,
			rx_tail: 0,
			tx_buffers: 0 as *mut TxBufferType,
			tx_descriptors: 0 as *mut TxDescriptorType,
			tx_tail: 0,
		}
	}

	fn flush(&self) {
		self.read_register(E1000_STATUS_REGISTER);
	}

	extern "x86-interrupt" fn interrupt_handler(_stack_frame: &mut irq::ExceptionStackFrame) {
		let e1000 = unsafe { ADAPTER.as_mut().unwrap() };
		let interface = unsafe { &mut EN0 };

		// Disable all interrupts.
		e1000.write_register(E1000_IMC_REGISTER, u32::MAX);
		e1000.flush();

		// Read the Interrupt Cause Read (ICR) register to clear it.
		// We have only enabled interrupt indicating packet receival, so we don't need to check the register.
		e1000.read_register(E1000_ICR_REGISTER);

		if !interface.is_receiving() {
			interface.start_receiving(Self::receive_handler);
		}

		apic::eoi();
	}

	unsafe extern "C" fn receive_handler(ctx: usize) {
		let e1000 = ADAPTER.as_mut().unwrap();
		let interface = &mut *(ctx as *mut NetworkInterface);

		loop {
			let descriptor = &mut (*e1000.rx_descriptors)[e1000.rx_tail];
			let status = ptr::read_volatile(&descriptor.status);
			let error = ptr::read_volatile(&descriptor.error);

			// Exit the loop as soon as we find the first unfinished packet.
			if status & E1000_RX_DESCRIPTOR_STATUS_DONE == 0 {
				break;
			}

			// Check if this is a finished packet without errors.
			if status & E1000_RX_DESCRIPTOR_STATUS_EOP > 0 && error == 0 {
				// It is, so let lwIP read it into a pbuf.
				let length = ptr::read_volatile(&descriptor.length) as usize;
				interface.receive_packet((*e1000.rx_buffers)[e1000.rx_tail].as_ptr(), length);
			}

			// Mark this buffer entry as unused again and advance the ring buffer pointer.
			ptr::write_volatile(&mut descriptor.status, 0);
			e1000.rx_tail = (e1000.rx_tail + 1) % NUM_RX_DESCRIPTORS;
			e1000.write_register(E1000_RDT_REGISTER, e1000.rx_tail as u32);
		}

		// We are done receiving, reenable our interrupts.
		interface.stop_receiving();
		e1000.write_register(E1000_IMS_REGISTER, INTERRUPT_MASK);
		e1000.flush();
	}

	unsafe extern "C" fn transmit_handler(_netif: *mut netif, p: *mut pbuf) -> err_t {
		let e1000 = ADAPTER.as_mut().unwrap();

		// Verify that this packet isn't larger than a transmission buffer entry.
		assert!(
			(*p).tot_len <= TX_BUFFER_SIZE as u16,
			"Trying to send a packet of {} bytes, but only {} bytes supported!",
			(*p).tot_len,
			TX_BUFFER_SIZE
		);

		// Let lwIP copy this pbuf into our transmission buffer entry.
		NetworkInterface::transmit_packet((*e1000.tx_buffers)[e1000.tx_tail].as_mut_ptr(), p);

		// Mark this buffer entry as ready and advance the ring buffer pointer.
		let descriptor = &mut (*e1000.tx_descriptors)[e1000.tx_tail];
		ptr::write_volatile(&mut descriptor.length, (*p).tot_len);
		ptr::write_volatile(&mut descriptor.status, 0);
		ptr::write_volatile(
			&mut descriptor.cmd,
			E1000_TX_DESCRIPTOR_CMD_EOP | E1000_TX_DESCRIPTOR_CMD_INSERT_FCS | E1000_TX_DESCRIPTOR_CMD_REPORT_STATUS
		);
		e1000.tx_tail = (e1000.tx_tail + 1) % NUM_TX_DESCRIPTORS;
		e1000.write_register(E1000_TDT_REGISTER, e1000.tx_tail as u32);

		ERR_OK as err_t
	}

	fn read_eeprom(&self, address: u8) -> u16 {
		self.write_register(E1000_EERD_REGISTER, ((address as u32) << E1000_EERD_ADDR_SHIFT) | E1000_EERD_START);

		loop {
			let data = self.read_register(E1000_EERD_REGISTER);
			if data & E1000_EERD_DONE > 0 {
				return (data >> E1000_EERD_DATA_SHIFT) as u16;
			}

			processor::udelay(1);
		}
	}

	fn read_register(&self, register: usize) -> u32 {
		let ptr = (self.base_address + register) as *mut u32;
		unsafe { ptr::read_volatile(ptr) }
	}

	fn write_register(&self, register: usize, value: u32) {
		let ptr = (self.base_address + register) as *mut u32;
		unsafe { ptr::write_volatile(ptr, value); }
	}

	extern "C" fn init(netif: *mut netif) -> err_t {
		let e1000 = unsafe { ADAPTER.as_mut().unwrap() };

		debug!(
			"Initializing Intel E1000 Ethernet Adapter [{:04X}:{:04X}] at {:02X}:{:02X}",
			e1000.pci_adapter.vendor_id,
			e1000.pci_adapter.device_id,
			e1000.pci_adapter.bus,
			e1000.pci_adapter.device
		);

		// Check the adapter's base address.
		let base_address = e1000.pci_adapter.base_addresses[0];
		assert!(base_address & pci::PCI_BASE_ADDRESS_IO_SPACE == 0, "Detected Intel E1000 uses I/O space, which is not supported.");
		assert!(base_address & pci::PCI_BASE_ADDRESS_64BIT == 0, "Detected Intel E1000 uses 64-bit Base Address, which is not supported.");

		e1000.base_address = (base_address & pci::PCI_BASE_ADDRESS_MASK) as usize;
		let base_size = e1000.pci_adapter.base_sizes[0] as usize;

		// Map the base address and mark it as reserved in the virtual memory free list.
		paging::map::<BasePageSize>(
			e1000.base_address,
			e1000.base_address,
			base_size / BasePageSize::SIZE,
			PageTableEntryFlags::WRITABLE | PageTableEntryFlags::CACHE_DISABLE | PageTableEntryFlags::EXECUTE_DISABLE,
			false
		);
		virtualmem::reserve(e1000.base_address, base_size);

		// Reset the adapter.
		e1000.write_register(E1000_CTRL_REGISTER, E1000_CTRL_RESET);
		e1000.flush();
		processor::udelay(10);

		// Configure the adapter.
		let mut ctrl = e1000.read_register(E1000_CTRL_REGISTER);
		ctrl |= E1000_CTRL_AUTO_SPEED_DETECTION_ENABLE | E1000_CTRL_SET_LINK_UP;
		ctrl &= !(E1000_CTRL_LINK_RESET | E1000_CTRL_INVERT_LOSS_OF_SIGNAL | E1000_CTRL_FORCE_SPEED | E1000_CTRL_VLAN_MODE_ENABLE | E1000_CTRL_PHY_RESET);
		e1000.write_register(E1000_CTRL_REGISTER, ctrl);
		e1000.flush();

		//
		// TRANSMITTER CONFIGURATION
		//
		// Disable the transmitter while we set it up.
		let mut tctl = e1000.read_register(E1000_TCTL_REGISTER);
		tctl &= !E1000_TCTL_ENABLE;
		e1000.write_register(E1000_TCTL_REGISTER, tctl);
		e1000.flush();

		// Initialize the TX buffer and descriptor memory.
		e1000.tx_buffers = mm::allocate(mem::size_of::<TxBufferType>(), PageTableEntryFlags::EXECUTE_DISABLE) as *mut TxBufferType;
		unsafe { ptr::write_bytes(e1000.tx_buffers, 0, 1); }

		e1000.tx_descriptors = mm::allocate(mem::size_of::<TxDescriptorType>(), PageTableEntryFlags::EXECUTE_DISABLE) as *mut TxDescriptorType;
		unsafe { ptr::write_bytes(e1000.tx_descriptors, 0, 1); }

		for i in 0..NUM_TX_DESCRIPTORS {
			unsafe {
				let descriptor = &mut ((*e1000.tx_descriptors)[i]);
				let buffer = &((*e1000.tx_buffers)[i]);
				descriptor.addr = paging::get_physical_address::<BasePageSize>(buffer as *const _ as usize) as u64;
				descriptor.status = E1000_TX_DESCRIPTOR_STATUS_DONE;
			}
		}

		// Make it known to the adapter.
		let tx_descriptors_physical_address = paging::get_physical_address::<BasePageSize>(e1000.tx_descriptors as usize);
		e1000.write_register(E1000_TDBAL_REGISTER, tx_descriptors_physical_address as u32);
		e1000.write_register(E1000_TDBAH_REGISTER, (tx_descriptors_physical_address >> 32) as u32);
		e1000.write_register(E1000_TDLEN_REGISTER, mem::size_of::<TxDescriptorType>() as u32);
		e1000.write_register(E1000_TDH_REGISTER, 0);
		e1000.write_register(E1000_TDT_REGISTER, 0);

		// Auto-Negotiation should have completed by now.
		// Check the link speed.
		let status = e1000.read_register(E1000_STATUS_REGISTER);

		let speed = if status & E1000_STATUS_SPEED_1000 > 0 {
			1000
		} else if status & E1000_STATUS_SPEED_100 > 0 {
			100
		} else {
			10
		};

		// Check whether this is a full or half-duplex link and set the according collision distance value.
		let (duplex, collision_distance) = if status & E1000_STATUS_FULL_DUPLEX > 0 {
			(&"Full", E1000_TCTL_COLLISION_DISTANCE_FULL_DUPLEX)
		} else {
			(&"Half", E1000_TCTL_COLLISION_DISTANCE_HALF_DUPLEX)
		};

		info!("Established {} MBit/s {} Duplex Link", speed, duplex);

		// Enable the transmitter.
		tctl = E1000_TCTL_ENABLE |
			E1000_TCTL_PAD_SHORT_PACKET |
			(E1000_TCTL_COLLISION_THRESHOLD_DEFAULT << E1000_TCTL_COLLISION_THRESHOLD_SHIFT) |
			(collision_distance << E1000_TCTL_COLLISION_DISTANCE_SHIFT);
		e1000.write_register(E1000_TCTL_REGISTER, tctl);
		e1000.flush();

		// Configure the Inter Packet Gap timer to the recommended settings.
		let tipg = E1000_TIPG_TRANSMIT_TIME_DEFAULT |
			(E1000_TIPG_RECEIVE_TIME1_DEFAULT << E1000_TIPG_RECEIVE_TIME1_SHIFT) |
			(E1000_TIPG_RECEIVE_TIME2_DEFAULT << E1000_TIPG_RECEIVE_TIME2_SHIFT);
		e1000.write_register(E1000_TIPG_REGISTER, tipg);

		// Read the MAC address.
		let mut eeprom_address: u8 = 0;
		let mut mac_address: [u8; NETIF_MAX_HWADDR_LEN] = [0; NETIF_MAX_HWADDR_LEN];

		while eeprom_address < (NETIF_MAX_HWADDR_LEN as u8) {
			let address_word = e1000.read_eeprom(eeprom_address);
			mac_address[eeprom_address as usize] = address_word as u8;
			eeprom_address += 1;
			mac_address[eeprom_address as usize] = (address_word >> 8) as u8;
			eeprom_address += 1;
		}

		// Configure the adapter to accept packets directed to our MAC address.
		let mut receive_address: u64 = E1000_RA_ADDRESS_VALID;
		for i in 0..NETIF_MAX_HWADDR_LEN {
			receive_address |= (mac_address[i] as u64) << (i * 2);
		}

		e1000.write_register(E1000_RAL_REGISTER, receive_address as u32);
		e1000.write_register(E1000_RAH_REGISTER, (receive_address >> 32) as u32);

		// Clear all other receive addresses.
		for i in 1..16 {
			e1000.write_register(E1000_RAL_REGISTER + i * 8, 0);
			e1000.write_register(E1000_RAH_REGISTER + i * 8, 0);
		}
		e1000.flush();

		// Clear the Multicast table.
		for i in 0..128 {
			e1000.write_register(E1000_MTA_REGISTER + i * 4, 0);
		}
		e1000.flush();

		//
		// RECEIVER CONFIGURATION
		//
		// Disable the receiver.
		let mut rctl = e1000.read_register(E1000_RCTL_REGISTER);
		rctl &= !E1000_RCTL_ENABLE;
		e1000.write_register(E1000_RCTL_REGISTER, rctl);
		e1000.flush();

		// Set our interrupt handler.
		let interrupt_number = pic::PIC1_INTERRUPT_OFFSET + e1000.pci_adapter.irq;
		idt::set_gate(interrupt_number, Self::interrupt_handler as usize, 1);

		// Disable all interrupts.
		e1000.write_register(E1000_IMS_REGISTER, 0xFFFF);
		e1000.flush();
		e1000.write_register(E1000_IMC_REGISTER, 0xFFFF);
		e1000.flush();

		// Enable the interrupts we are interested in.
		e1000.write_register(E1000_IMS_REGISTER, INTERRUPT_MASK);
		e1000.flush();

		// Clear all outstanding interrupts.
		e1000.read_register(E1000_ICR_REGISTER);

		// Initialize the RX buffer and descriptor memory.
		e1000.rx_buffers = mm::allocate(mem::size_of::<RxBufferType>(), PageTableEntryFlags::EXECUTE_DISABLE) as *mut RxBufferType;
		unsafe { ptr::write_bytes(e1000.rx_buffers, 0, 1); }

		e1000.rx_descriptors = mm::allocate(mem::size_of::<RxDescriptorType>(), PageTableEntryFlags::EXECUTE_DISABLE) as *mut RxDescriptorType;
		unsafe { ptr::write_bytes(e1000.rx_descriptors, 0, 1); }

		for i in 0..NUM_RX_DESCRIPTORS {
			unsafe {
				let descriptor = &mut (*e1000.rx_descriptors)[i];
				let buffer = &(*e1000.rx_buffers)[i];
				descriptor.addr = paging::get_physical_address::<BasePageSize>(buffer as *const _ as usize) as u64;
			}
		}

		// Make it known to the adapter.
		let rx_descriptors_physical_address = paging::get_physical_address::<BasePageSize>(e1000.rx_descriptors as usize);
		e1000.write_register(E1000_RDBAL_REGISTER, rx_descriptors_physical_address as u32);
		e1000.write_register(E1000_RDBAH_REGISTER, (rx_descriptors_physical_address >> 32) as u32);
		e1000.write_register(E1000_RDLEN_REGISTER, mem::size_of::<RxDescriptorType>() as u32);
		e1000.write_register(E1000_RDH_REGISTER, 0);
		e1000.write_register(E1000_RDT_REGISTER, 0);

		// Enable the receiver.
		rctl = E1000_RCTL_ENABLE | E1000_RCTL_BROADCAST_ACCEPT | E1000_RCTL_STRIP_ETHERNET_CRC;
		e1000.write_register(E1000_RCTL_REGISTER, rctl);
		e1000.flush();

		//
		// LWIP INTERFACE CONFIGURATION
		//
		let interface = unsafe { &mut *((*netif).state as *mut NetworkInterface) };
		interface.set_linkoutput_handler(Self::transmit_handler);
		interface.set_mac_address(mac_address);

		ERR_OK as err_t
	}
}

pub fn detect() -> DetectionResult {
	// Find the first supported network adapter on the PCI bus.
	for &(vendor_id, device_id) in SUPPORTED_DEVICES {
		if let Some(pci_adapter) = pci::get_adapter(vendor_id, device_id) {
			return Some((pci_adapter, init));
		}
	}

	None
}

fn init(netif: &mut netif, pci_adapter: pci::PciAdapter, ip: ip_addr_t, netmask: ip_addr_t, gateway: ip_addr_t) {
	unsafe {
		ADAPTER = Some(Box::new(E1000NetworkAdapter::new(pci_adapter)));
		assert!(netifapi_netif_add(netif, &ip, &netmask, &gateway, 0, E1000NetworkAdapter::init, ethernet_input) == ERR_OK as err_t);
	}
}
