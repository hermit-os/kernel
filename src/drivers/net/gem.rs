//! Driver for the Cadence Gigabit Ethernet MAC (GEM) on the Freedom U740 SoC.
//!
//! The driver is derived from the Zynq 7000 Gigabit Ethernet Controller (GEM) reference manual.
//! See the [Zynq 7000 SoC Technical Reference Manual (UG585)](https://docs.xilinx.com/r/en-US/ug585-zynq-7000-SoC-TRM) for more details.

#![allow(unused)]

use alloc::vec::Vec;
use core::convert::TryInto;
use core::{mem, slice};

use riscv::register::*;
use tock_registers::interfaces::*;
use tock_registers::registers::*;
use tock_registers::{register_bitfields, register_structs};

use crate::arch::kernel::core_local::core_scheduler;
use crate::arch::kernel::interrupts::*;
use crate::arch::kernel::pci;
use crate::arch::mm::paging::virt_to_phys;
use crate::arch::mm::VirtAddr;
use crate::drivers::error::DriverError;
use crate::drivers::net::{network_irqhandler, NetworkDriver};
use crate::executor::device::{RxToken, TxToken};

//Base address of the control registers
//const GEM: *mut Registers = 0x1009_0000 as *mut Registers; //For Sifive FU540
//const GEM_IRQ: u32 = 53; //For Sifive FU540

// https://github.com/torvalds/linux/blob/v4.15/drivers/net/ethernet/cadence/macb.h
register_structs! {
	/// Register offsets
	Registers {
		// Control register: read-write
		(0x000 => network_control: ReadWrite<u32, NetworkControl::Register>),
		(0x004 => network_config: ReadWrite<u32, NetworkConfig::Register>),
		(0x008 => network_status: ReadOnly<u32, NetworkStatus::Register>),
		(0x00C => _reserved1),
		(0x010 => dma_config: ReadWrite<u32, DMAConfig::Register>),
		(0x014 => transmit_status: ReadWrite<u32, TransmitStatus::Register>),
		(0x018 => rx_qbar: ReadWrite<u32>),
		(0x01c => tx_qbar: ReadWrite<u32>),
		(0x020 => receive_status: ReadWrite<u32, RecieveStatus::Register>),
		(0x024 => int_status: ReadWrite<u32, Interrupts::Register>),
		(0x028 => int_enable: WriteOnly<u32, Interrupts::Register>),
		(0x02C => int_disable: WriteOnly<u32, Interrupts::Register>),
		(0x030 => _reserved3),
		(0x034 => phy_maintenance: ReadWrite<u32, PHYMaintenance::Register>),
		(0x038 => _reserved4),
		(0x088 => spec_add1_bottom: ReadWrite<u32>),
		(0x08C => spec_add1_top: ReadWrite<u32>),
		(0x090 => _reserved5),
		(0x1000 => @END),
	}
}

register_bitfields! [
	// First parameter is the register width. Can be u8, u16, u32, or u64.
	u32,

	NetworkControl [
		STARTTX	OFFSET(9) NUMBITS(1) [],
		STATCLR	OFFSET(5) NUMBITS(1) [],
		MDEN	OFFSET(4) NUMBITS(1) [],
		TXEN	OFFSET(3) NUMBITS(1) [],
		RXEN	OFFSET(2) NUMBITS(1) [],
	],
	NetworkConfig [
		RXCHKSUMEN	OFFSET(24) NUMBITS(1) [],
		DBUS_WIDTH	OFFSET(21) NUMBITS(2) [
			DBW32 = 0,
			DBW64 = 1,
			DBW128 = 2
		],
		MDCCLKDIV 	OFFSET(18) NUMBITS(3) [
			CLK_DIV8 = 0,
			CLK_DIV16 = 1,
			CLK_DIV32 = 2,
			CLK_DIV48 = 3,
			CLK_DIV64 = 4,
			CLK_DIV96 = 5,
			CLK_DIV128 = 6,
			CLK_DIV224 = 7
		],
		FCSREM		OFFSET(17) NUMBITS(1) [],
		PAUSEEN		OFFSET(13) NUMBITS(1) [],
		GIGEEN		OFFSET(10) NUMBITS(1) [],
		MCASTHASHEN	OFFSET(6) NUMBITS(1) [],
		BCASTDI		OFFSET(5) NUMBITS(1) [],
		COPYALLEN	OFFSET(4) NUMBITS(1) [],
		FDEN OFFSET(1) NUMBITS(1) [],
	],
	NetworkStatus [
		PHY_MGMT_IDLE	OFFSET(2) NUMBITS(1) [],
	],
	DMAConfig [
		RXBUF		OFFSET(16) NUMBITS(8) [],
		TCPCKSUM	OFFSET(11) NUMBITS(1) [],
		TXSIZE		OFFSET(10) NUMBITS(1) [],
		RXSIZE		OFFSET(8) NUMBITS(2) [
			//Supported on all devices?
			FULL_ADDRESSABLE_SPACE = 3
		],
		ENDIAN		OFFSET(7) NUMBITS(1) [],
		BLENGTH		OFFSET(0) NUMBITS(5) [
			SINGLE = 0b00001,
			INCR4 = 0b00100,
			INCR8 = 0b01000,
			INCR16 = 0b10000
		],
	],
	RecieveStatus [
		FRAMERX  OFFSET(1) NUMBITS(1) [],
	],
	TransmitStatus [
		TXCOMPL	OFFSET(5) NUMBITS(1) [],
		TXGO	OFFSET(3) NUMBITS(1) [],
	],
	Interrupts [
		TSU_SEC_INCR	OFFSET(26) NUMBITS(1) [],
		TXCOMPL			OFFSET(7) NUMBITS(1) [],
		FRAMERX			OFFSET(1) NUMBITS(1) [],

	],
	PHYMaintenance [
		CLAUSE_22	OFFSET(30) NUMBITS(1) [],
		OP			OFFSET(28) NUMBITS(2) [
			READ = 0b10,
			WRITE = 0b01,
		],
		ADDR		OFFSET(23) NUMBITS(5) [],
		REG			OFFSET(18) NUMBITS(5) [],
		MUST_10		OFFSET(16) NUMBITS(2) [
			MUST_BE_10 = 0b10
		],
		DATA		OFFSET(0) NUMBITS(16) [],
	],
];

///  PHY reg index
enum PhyReg {
	Control = 0,
	Status = 1,
	ID1 = 2,
	ID2 = 3,
	ANAdvertisement = 4,
	ANLinkPartnerAbility = 5,
	ANExpansion = 6,
	ANNextPageTransmit = 7,
	ANLinkPartnerReceivedNextPage = 8,
	ExtendedStatus = 15,
}

///  PHY Status reg mask and offset
enum PhyStatus {
	ANCompleteOffset = 5,
	ANCompleteMask = 0x20,
	ANCapOffset = 3,
	ANCapMask = 0x4,
}

enum PhyControl {
	ANEnableOffset = 12,
	ANEnableMask = 0x1000,
}

enum PhyPartnerAbility {
	ANEnableOffset = 12,
	ANEnableMask = 0x1000,
}

/// size of a receive buffer (must be multiple of 64)
const RX_BUF_LEN: u32 = 1600;
const RX_BUFFER_MULTIPLE: u32 = 64;
/// Number of receive buffers
const RX_BUF_NUM: u32 = 64;

/// size of a transmit buffer
const TX_BUF_LEN: u32 = 1600;
/// Number of transmit buffers
const TX_BUF_NUM: u32 = 1;

/// Marks tx buffer as last buffer of frame
const TX_DESC_LAST: u32 = 1 << 15;

/// Marks tx buffer wrap buffer
const TX_DESC_WRAP: u32 = 1 << 30;

/// Marks tx buffer as used
const TX_DESC_USED: u32 = 1 << 31;

#[derive(Debug)]
pub enum GEMError {
	InitFailed,
	ResetFailed,
	NoPhyFound,
	Unknown,
}

/// GEM network driver struct.
///
/// Struct allows to control device queus as also
/// the device itself.
pub struct GEMDriver {
	// Pointer to the registers of the controller
	gem: *mut Registers,
	mtu: u16,
	irq: u8,
	mac: [u8; 6],
	rx_counter: u32,
	rxbuffer: VirtAddr,
	rxbuffer_list: VirtAddr,
	tx_counter: u32,
	txbuffer: VirtAddr,
	txbuffer_list: VirtAddr,
}

impl NetworkDriver for GEMDriver {
	/// Returns the MAC address of the network interface
	fn get_mac_address(&self) -> [u8; 6] {
		self.mac
	}

	/// Returns the current MTU of the device.
	fn get_mtu(&self) -> u16 {
		self.mtu
	}

	fn send_packet<R, F>(&mut self, len: usize, f: F) -> R
	where
		F: FnOnce(&mut [u8]) -> R,
	{
		debug!("get_tx_buffer");

		if len as u32 > TX_BUF_LEN {
			panic!("TX buffer is too small");
		}

		self.handle_interrupt();

		for i in 0..TX_BUF_NUM {
			let index = (i + self.tx_counter) % TX_BUF_NUM;
			let word1_addr = (self.txbuffer_list + (index * 8 + 4) as u64).as_mut_ptr::<u32>();
			let word1 = unsafe { core::ptr::read_volatile(word1_addr) };
			// Reuse a used buffer
			if word1 & TX_DESC_USED != 0 {
				// Clear used bit
				unsafe {
					core::ptr::write_volatile(word1_addr, word1 & (!TX_DESC_USED));
				}

				// Set new starting point to search for next buffer
				self.tx_counter = (index + 1) % TX_BUF_NUM;

				// Address of the tx buffer
				let buffer = (self.txbuffer + (index * TX_BUF_LEN) as u64).as_mut_ptr::<u8>();
				let buffer = unsafe { slice::from_raw_parts_mut(buffer, len) };
				let result = f(buffer);

				debug!("send_tx_buffer");

				// Address of word[1] of the buffer descriptor
				let word1_addr = (self.txbuffer_list + (index * 8 + 4) as u64).as_mut_ptr::<u32>();
				let word1 = unsafe { core::ptr::read_volatile(word1_addr) };

				unsafe {
					// Set length of frame and mark as single buffer Ethernet frame
					core::ptr::write_volatile(
						word1_addr,
						(word1 & TX_DESC_WRAP) | TX_DESC_LAST | len as u32,
					);

					// Enable TX
					(*self.gem)
						.network_control
						.modify(NetworkControl::TXEN::SET);
					// Start transmission
					(*self.gem)
						.network_control
						.modify(NetworkControl::STARTTX::SET);

					// (*GEM).network_control.modify(NetworkControl::RXEN::CLEAR);
				}

				// Set used bit to indicate that the buffer can be reused
				let word1_addr = (self.txbuffer_list + (index * 8 + 4) as u64).as_mut_ptr::<u32>();
				let word1 = unsafe { core::ptr::read_volatile(word1_addr) };
				unsafe {
					core::ptr::write_volatile(word1_addr, word1 | TX_DESC_USED);
				}

				return result;
			}
		}

		panic!("Unable to get TX buffer")
	}

	fn has_packet(&self) -> bool {
		debug!("has_packet");

		match self.next_rx_index() {
			Some(_) => true,
			None => false,
		}
	}

	fn receive_packet(&mut self) -> Option<(RxToken, TxToken)> {
		debug!("receive_rx_buffer");

		// Scan the buffer descriptor queue starting from rx_count
		match self.next_rx_index() {
			Some(index) => {
				let word1_addr = (self.rxbuffer_list + (index * 8 + 4) as u64);
				let word1_entry =
					unsafe { core::ptr::read_volatile(word1_addr.as_mut_ptr::<u32>()) };
				let length = word1_entry & 0x1FFF;
				debug!("Recieved frame in buffer {}, length: {}", index, length);

				// Starting point to search for next frame
				self.rx_counter = (index + 1) % RX_BUF_NUM;
				// SAFETY: This is a blatant lie and very unsound.
				// The API must be fixed or the buffer may never touched again.
				let buffer = unsafe {
					core::slice::from_raw_parts_mut(
						(self.rxbuffer.as_usize() + (index * RX_BUF_LEN) as usize) as *const u8
							as *mut u8,
						length as usize,
					)
				};
				trace!("BUFFER: {:x?}", buffer);
				self.rx_buffer_consumed(index as usize);
				Some((RxToken::new(buffer.to_vec()), TxToken::new()))
			}
			None => None,
		}
	}

	fn set_polling_mode(&mut self, value: bool) {
		debug!("set_polling_mode");
		if value {
			// disable interrupts from the NIC
			unsafe {
				(*self.gem).int_disable.set(0x7FF_FEFF);
			}
		} else {
			// Enable all known interrupts by setting the interrupt mask.
			unsafe {
				(*self.gem).int_enable.write(Interrupts::FRAMERX::SET);
			}
		}
	}

	fn handle_interrupt(&mut self) -> bool {
		let int_status = unsafe { (*self.gem).int_status.extract() };

		let receive_status = unsafe { (*self.gem).receive_status.extract() };

		let transmit_status = unsafe { (*self.gem).transmit_status.extract() };

		debug!(
			"handle_interrupt\nint_status: {:?}\nreceive_status: {:?}\ntransmit_status: {:?}",
			int_status, receive_status, transmit_status
		);

		if transmit_status.is_set(TransmitStatus::TXCOMPL) {
			debug!("TX COMPLETE");
			unsafe {
				(*self.gem)
					.int_status
					.modify_no_read(int_status, Interrupts::TXCOMPL::SET);
				(*self.gem)
					.transmit_status
					.modify_no_read(transmit_status, TransmitStatus::TXCOMPL::SET);
				(*self.gem)
					.network_control
					.modify(NetworkControl::TXEN::CLEAR);
			}
		}

		let ret =
			int_status.is_set(Interrupts::FRAMERX) && receive_status.is_set(RecieveStatus::FRAMERX);

		if ret {
			debug!("RX COMPLETE");
			unsafe {
				(*self.gem)
					.int_status
					.modify_no_read(int_status, Interrupts::FRAMERX::SET);
				(*self.gem)
					.receive_status
					.modify_no_read(receive_status, RecieveStatus::FRAMERX::SET);
			}

			// handle incoming packets
			todo!();
		}
		// increment_irq_counter((32 + self.irq).into());
		ret
	}
}

impl GEMDriver {
	// Tells driver, that buffer is consumed and can be deallocated
	fn rx_buffer_consumed(&mut self, handle: usize) {
		debug!("rx_buffer_consumed: handle: {}", handle);

		let word0_addr = (self.rxbuffer_list + (handle * 8) as u64);
		let word1_addr = word0_addr + 4 as u64;

		unsafe {
			// Clear word1 (is this really necessary?)
			core::ptr::write_volatile(word1_addr.as_mut_ptr::<u32>(), 0);
			// Give back ownership to GEM
			let word0_entry = core::ptr::read_volatile(word0_addr.as_mut_ptr::<u32>());
			core::ptr::write_volatile(word0_addr.as_mut_ptr::<u32>(), word0_entry & 0xFFFF_FFFE);
		}
	}

	/// Returns the index of the next recieved frame
	fn next_rx_index(&self) -> Option<u32> {
		// Scan the buffer descriptor queue starting from rx_count

		for i in 0..RX_BUF_NUM {
			let index = (i + self.rx_counter) % RX_BUF_NUM;
			let word0_addr = (self.rxbuffer_list + (index * 8) as u64);
			let word0_entry = unsafe { core::ptr::read_volatile(word0_addr.as_mut_ptr::<u32>()) };
			// Is buffer owned by GEM?
			if (word0_entry & 0x1) != 0 {
				return Some(index);
			}
		}

		None
	}
}

impl Drop for GEMDriver {
	fn drop(&mut self) {
		debug!("Dropping GEMDriver!");

		// Software reset
		// Clear the Network Control register
		unsafe {
			(*self.gem).network_control.set(0x0);
		}

		crate::mm::deallocate(self.rxbuffer, (RX_BUF_LEN * RX_BUF_NUM) as usize);
		crate::mm::deallocate(self.txbuffer, (TX_BUF_LEN * TX_BUF_NUM) as usize);
		crate::mm::deallocate(self.rxbuffer_list, (8 * RX_BUF_NUM) as usize);
		crate::mm::deallocate(self.txbuffer_list, (8 * TX_BUF_NUM) as usize);
	}
}

/// Inits the driver. Passing u32::MAX as phy_addr will trigger a search for the actual PHY address
pub fn init_device(
	gem_base: VirtAddr,
	irq: u8,
	phy_addr: u32,
	mac: [u8; 6],
) -> Result<GEMDriver, DriverError> {
	debug!("Init GEM at {:p}", gem_base);

	let gem = gem_base.as_mut_ptr::<Registers>();

	unsafe {
		// Initialize the Controller

		// Clear the Network Control register
		(*gem).network_control.set(0x0);
		// Clear the Statistics registers
		(*gem).network_control.modify(NetworkControl::STATCLR::SET);
		// Clear the status registers
		(*gem).receive_status.set(0x0F);
		(*gem).transmit_status.set(0x0F);
		// Disable all interrupts
		(*gem).int_disable.set(0x7FF_FEFF);
		// Clear the buffer queues
		(*gem).rx_qbar.set(0x0);
		(*gem).tx_qbar.set(0x0);

		// Configure the Controller

		// Enable Full Duplex
		(*gem).network_config.modify(NetworkConfig::FDEN::SET);
		// Enable Gigabit mode
		(*gem).network_config.modify(NetworkConfig::GIGEEN::SET);
		// Enable reception of broadcast or multicast frames
		(*gem)
			.network_config
			.modify(NetworkConfig::BCASTDI::CLEAR + NetworkConfig::MCASTHASHEN::SET);
		// Enable promiscuous mode
		// (*GEM).network_config.modify(NetworkConfig::COPYALLEN::SET);
		// Enable TCP/IP checksum offload feature on receive
		(*gem).network_config.modify(NetworkConfig::RXCHKSUMEN::SET);
		// Enable Pause frames
		(*gem).network_config.modify(NetworkConfig::PAUSEEN::SET);
		// Set the MDC clock divisor
		//(CLK_DIV64 for up to 160 Mhz) TODO: Determine the correct value
		(*gem)
			.network_config
			.modify(NetworkConfig::MDCCLKDIV::CLK_DIV64);
		// Enable FCS remove
		(*gem).network_config.modify(NetworkConfig::FCSREM::SET);

		// Set the MAC address
		let bottom: u32 = ((mac[3] as u32) << 24)
			+ ((mac[2] as u32) << 16)
			+ ((mac[1] as u32) << 8)
			+ ((mac[0] as u32) << 0);
		let top: u32 = ((mac[5] as u32) << 8) + ((mac[4] as u32) << 0);
		(*gem).spec_add1_bottom.set(bottom);
		(*gem).spec_add1_top.set(top);

		// Program the DMA configuration register

		// Set the receive buffer size (TODO: Jumbo packet support)
		(*gem)
			.dma_config
			.modify(DMAConfig::RXBUF.val(RX_BUF_LEN / RX_BUFFER_MULTIPLE));
		// Set the receiver packet buffer memory size to the full configured addressable space
		(*gem)
			.dma_config
			.modify(DMAConfig::RXSIZE::FULL_ADDRESSABLE_SPACE);
		// Set the transmitter packet buffer memory size to the full configured addressable space
		(*gem).dma_config.modify(DMAConfig::TXSIZE::SET);
		// Enable TCP/IP checksum generation offload on the transmitter
		(*gem).dma_config.modify(DMAConfig::TCPCKSUM::SET);
		// Configure for Little Endian system
		(*gem).dma_config.modify(DMAConfig::ENDIAN::CLEAR);
		// Configure fixed burst length to INCR16
		(*gem).dma_config.modify(DMAConfig::BLENGTH::INCR16);

		// Program the Network Control Register

		// Enable MDIO and enable transmitter/receiver
		// (*gem).network_control.modify(
		// 	NetworkControl::MDEN::SET + NetworkControl::TXEN::SET + NetworkControl::RXEN::SET,
		// );
		(*gem).network_control.modify(NetworkControl::MDEN::SET);

		// PHY Initialization
		let mut phy_addr = phy_addr;
		if phy_addr == u32::MAX {
			// Detect PHY
			warn! {"No PHY address provided. Trying to find PHY ..."}
			for i in 0..32 {
				match phy_read(gem, i, PhyReg::Control) {
					0xFFFF => (), //Invalid
					0x0 => (),    //Invalid
					_ => {
						phy_addr = i;
						warn!("PHY found with address {}", phy_addr);
						break;
					}
				}
				if i == 31 {
					error!("No PHY found");
					return Err(DriverError::InitGEMDevFail(GEMError::NoPhyFound));
				}
			}
		}

		// Clause 28 auto-negotiation https://opencores.org/websvn/filedetails?repname=1000base-x&path=%2F1000base-x%2Ftrunk%2Fdoc%2F802.3-2008_section2.pdf&bcsi_scan_91c2e97ef32f18a3=V1Ygi7liGXdis80J3CYk1MUlxZsSAAAACY4+BA%3D%3D+
		// This is PHY specific and may not work on all PHYs
		let phy_status = phy_read(gem, phy_addr, PhyReg::Status);

		// Chck for auto-negotiation ability
		if (phy_status & PhyStatus::ANCapMask as u16) == 0 {
			warn!("PHY does not support auto-negotiation");
		// TODO
		//return Err(DriverError::InitGEMDevFail(GEMError::NoPhyFound));
		} else {
			// Keep default values in Auto-Negotiation advertisement register
			// Enable AN
			let phy_control = phy_read(gem, phy_addr, PhyReg::Control);
			phy_write(
				gem,
				phy_addr,
				PhyReg::Control,
				PhyControl::ANEnableMask as u16 | phy_control,
			);

			// Wait for AN to complete
			while (phy_read(gem, phy_addr, PhyReg::Status) | PhyStatus::ANCompleteMask as u16) == 0
			{
			}

			// Read partner ability register
			let partner_ability = phy_read(gem, phy_addr, PhyReg::ANLinkPartnerAbility);

			// Get the supported Speed and Duplex
			// TODO - Next Page does not seem to be emulated by QEMU

			//info!("PHY auto-negotiation completed:\n Speed: {}\nDuplex", ,);
			debug!(
				"PHY auto-negotiation completed: Partner Ability {:x}",
				partner_ability
			);
		}
	}

	// Configure the Buffer Descriptors

	// Allocate Receive Buffer
	let rxbuffer = crate::mm::allocate((RX_BUF_LEN * RX_BUF_NUM) as usize, true);
	// Allocate Receive Buffer Descriptor List
	let rxbuffer_list = crate::mm::allocate((8 * RX_BUF_NUM) as usize, true);
	// Allocate Transmit Buffer
	let txbuffer = crate::mm::allocate((TX_BUF_LEN * TX_BUF_NUM) as usize, true);
	// Allocate Transmit Buffer Descriptor List
	let txbuffer_list = crate::mm::allocate((8 * TX_BUF_NUM) as usize, true);

	if txbuffer.is_zero()
		|| rxbuffer.is_zero()
		|| rxbuffer_list.is_zero()
		|| txbuffer_list.is_zero()
	{
		error!("Unable to allocate buffers for GEM");
		return Err(DriverError::InitGEMDevFail(GEMError::Unknown));
	}

	debug!(
		"Allocate TxBuffer at 0x{:x} and RxBuffer at 0x{:x}",
		txbuffer, rxbuffer
	);

	unsafe {
		// Init Receive Buffer Descriptor List
		for i in 0..RX_BUF_NUM {
			let word0 = (rxbuffer_list + (i * 8) as u64).as_mut_ptr::<u32>();
			let word1 = (rxbuffer_list + (i * 8 + 4) as u64).as_mut_ptr::<u32>();
			let buffer = virt_to_phys(rxbuffer + (i * RX_BUF_LEN) as u64);
			if (buffer.as_u64() & 0b11) != 0 {
				error!("Wrong buffer alignment");
				return Err(DriverError::InitGEMDevFail(GEMError::Unknown));
			}
			// This can fail if address of buffers is > 32 bit
			// TODO: 64-bit addresses
			let mut word0_entry: u32 = buffer.as_u64().try_into().unwrap();

			// Mark the last descriptor in the buffer descriptor list with the wrap bit
			if i == RX_BUF_NUM - 1 {
				word0_entry |= 0b10;
			}
			core::ptr::write_volatile(word0, word0_entry);
			core::ptr::write_volatile(word1, 0x0);
		}

		let rx_qbar: u32 = virt_to_phys(rxbuffer_list).as_u64().try_into().unwrap();
		debug!("Set rx_qbar to {:x}", rx_qbar);
		(*gem).rx_qbar.set(rx_qbar);

		// Init Transmit Buffer Descriptor List
		for i in 0..TX_BUF_NUM {
			let word0 = (txbuffer_list + (i * 8) as u64).as_mut_ptr::<u32>();
			let word1 = (txbuffer_list + (i * 8 + 4) as u64).as_mut_ptr::<u32>();
			let buffer = virt_to_phys(txbuffer + (i * TX_BUF_LEN) as u64);

			// This can fail if address of buffers is > 32 bit
			// TODO: 64-bit addresses
			let mut word0_entry: u32 = buffer.as_u64().try_into().unwrap();
			let mut word1_entry: u32 = TX_DESC_USED;
			// Mark the last descriptor in the buffer descriptor list with the wrap bit
			if i == TX_BUF_NUM - 1 {
				word1_entry |= TX_DESC_WRAP;
			}
			core::ptr::write_volatile(word0, word0_entry);
			core::ptr::write_volatile(word1, word1_entry);
		}

		let tx_qbar: u32 = virt_to_phys(txbuffer_list).as_u64().try_into().unwrap();
		debug!("Set tx_qbar to {:x}", tx_qbar);
		(*gem).tx_qbar.set(tx_qbar);

		// Configure Interrupts
		debug!(
			"Install interrupt handler for GEM at {:x}",
			network_irqhandler as usize
		);
		irq_install_handler(irq, network_irqhandler);
		(*gem).int_enable.write(Interrupts::FRAMERX::SET); // + Interrupts::TXCOMPL::SET

		// Enable the Controller (again?)
		// Enable the transmitter
		(*gem).network_control.modify(NetworkControl::TXEN::SET);
		// Enable the receiver
		(*gem).network_control.modify(NetworkControl::RXEN::SET);
	}

	debug!(
		"MAC address {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
		mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]
	);

	Ok(GEMDriver {
		gem: gem,
		mtu: 1500,
		irq: irq,
		mac: mac,
		rx_counter: 0,
		rxbuffer: rxbuffer,
		rxbuffer_list: rxbuffer_list,
		tx_counter: 0,
		txbuffer: txbuffer,
		txbuffer_list: txbuffer_list,
	})
}

unsafe fn phy_read(gem: *mut Registers, addr: u32, reg: PhyReg) -> u16 {
	// Check that no MDIO operation is in progress
	wait_for_mdio(gem);
	// Initiate the data shift operation over MDIO
	(*gem).phy_maintenance.write(
		PHYMaintenance::CLAUSE_22::SET
			+ PHYMaintenance::OP::READ
			+ PHYMaintenance::ADDR.val(addr)
			+ PHYMaintenance::REG.val(reg as u32)
			+ PHYMaintenance::MUST_10::MUST_BE_10,
	);
	wait_for_mdio(gem);
	(*gem).phy_maintenance.read(PHYMaintenance::DATA) as u16
}

unsafe fn phy_write(gem: *mut Registers, addr: u32, reg: PhyReg, data: u16) {
	// Check that no MDIO operation is in progress
	wait_for_mdio(gem);
	// Initiate the data shift operation over MDIO
	(*gem).phy_maintenance.write(
		PHYMaintenance::CLAUSE_22::SET
			+ PHYMaintenance::OP::WRITE
			+ PHYMaintenance::ADDR.val(addr)
			+ PHYMaintenance::REG.val(reg as u32)
			+ PHYMaintenance::MUST_10::MUST_BE_10
			+ PHYMaintenance::DATA.val(data as u32),
	);
	wait_for_mdio(gem);
}

unsafe fn wait_for_mdio(gem: *mut Registers) {
	// Check that no MDIO operation is in progress
	while !(*gem).network_status.is_set(NetworkStatus::PHY_MGMT_IDLE) {}
}
