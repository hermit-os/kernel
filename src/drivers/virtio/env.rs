// Copyright (c) 2020 Frederik Schulz, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

//! A module containing all environment specific funtion calls.
//! 
//! The module should easy partability of the code. Furthermore it provides
//! a clean boundary between virtio and the rest of the kernel. One additional aspect is to 
//! ensure only a single location needs changes, in cases where the underlying kernel code is changed

pub mod memory {
    use core::ops::Add;
    use core::mem;

    /// A newtype representing a memory offset wich can be used to be added to [PhyMemAddr](PhyMemAddr) or
    /// to [VirtMemAddr](VirtMemAddr). 
    #[derive(Copy, Clone, Debug)]
    pub struct Offset(usize);

    // INFO: In case Offset is change to supporrt other than 64 bit systems one also needs to adjust 
    // the respective From<Offset> for u32 implementation.
    impl From<u32> for Offset {
        fn from(val: u32 ) -> Self {
            match mem::size_of::<usize>() {
                4 => Offset(val as usize),
                _ => panic!("Currently only support for 32 offsets is given!"),
            }
        }
    }

    impl From<Offset> for u32 {
        fn from(val: Offset) -> u32 {
            // Check if Offset is not larger than 32 bit
            match mem::size_of::<Offset>() {
                4 => val.0 as u32,
                _ => panic!("Missing support for conversions from others than 32 bit usize."),
            }
        }
    }

    /// A newtype representing a virtual mempory address.
    #[derive(Copy, Clone, Debug)]
    pub struct VirtMemAddr(usize);

    impl From<u32> for VirtMemAddr {
        fn from(addr: u32) -> Self {
            match mem::size_of::<usize>() {
                4 => VirtMemAddr(addr as usize),
                8 => VirtMemAddr(addr as usize),
                _ => panic!("Currently only support for 32 and 64 bit machines given!"),
            }
        }
    }

    impl From<u64> for VirtMemAddr {
        fn from(addr: u64) -> Self {
            match mem::size_of::<usize>() {
                4 => panic!("Using a 64 bit address inside a 32 bit system!"),
                8 => VirtMemAddr(addr as usize),
                _ => panic!("Currently only support for 32 and 64 bit machines given!"),
            }
        }
    }

    impl From<usize> for VirtMemAddr {
        fn from (addr: usize) -> Self {
            VirtMemAddr(addr)
        }
    }

    impl Add<Offset> for VirtMemAddr {
        type Output = VirtMemAddr;

        fn add(self, other: Offset) -> Self::Output {
            VirtMemAddr(self.0 + other.0)
        } 
    }

    /// A newtype representing a physical memory address
    pub struct PhyMemAddr(usize);

    impl From<u32> for PhyMemAddr {
        fn from(addr: u32) -> Self {
            match mem::size_of::<usize>() {
                4 => PhyMemAddr(addr as usize),
                8 => PhyMemAddr(addr as usize),
                _ => panic!("Currently only support for 32 and 64 bit machines given!"),
            }
        }
    }

    impl From<u64> for PhyMemAddr {
        fn from(addr: u64) -> Self {
            match mem::size_of::<usize>() {
                4 => panic!("Using a 64 bit address inside a 32 bit system!"),
                8 => PhyMemAddr(addr as usize),
                _ => panic!("Currently only support for 32 and 64 bit machines given!"),
            }
        }
    }

    impl From<usize> for PhyMemAddr {
        fn from(addr: usize) -> Self {
            PhyMemAddr(addr)
        }
    }

    impl Add<Offset> for PhyMemAddr {
        type Output = PhyMemAddr;

        fn add(self, other: Offset) -> Self::Output {
            PhyMemAddr(self.0 + other.0)
        } 
    }
}

/// This module is used as a single entry point from Virtio code into 
/// other parts of the kernel. 
///
/// INFO: Values passed on to PCI devices are automatically converted into little endian
/// coding. Values provided from PCI devices are passed as native endian values. 
/// Meaning they are converted into big endian values on big endian machines and 
/// are not changed on little endian machines.
pub mod pci {
    use drivers::virtio::env::memory::{VirtMemAddr};
    use drivers::virtio::transport::pci::PciBar as VirtioPciBar;
    use drivers::virtio::types::Le32;
    use arch::x86_64::kernel::pci;
    use arch::x86_64::kernel::pci::{PciAdapter, PciBar};
    use arch::x86_64::kernel::pci::error::PciError;
    use alloc::vec::Vec;
    use core::result::Result;

    /// Wrapper function to read the configuration space of a PCI 
    /// device at the given register. Returns the registers value.
    ///
    /// WARN: Return value is little endian coded, if interpreted as multi-byte value.
    pub fn read_config(adapter: &PciAdapter, register: Le32) -> u32 {
        pci::read_config(adapter.bus, adapter.device, register.as_le_u32())
    }

    /// Wrapper function to write the configuraiton space of a PCI
    /// device at the given register.
    pub fn write_config(adapter: &PciAdapter, register: Le32, data: Le32) {
        pci::write_config(adapter.bus, adapter.device, register.as_le_u32(), data.as_le_u32());
    }


    /// Maps all memeory areas indicated by the devices BAR's into 
    /// Virtual address space. 
    ///
    /// As this function uses parts of the kernel pci code it is 
    /// outsourced into the env::pci module.
    /// 
    /// WARN: Currently unsafely casts kernel::PciBar.size (usize) to an 
    /// u64
    pub fn map_bar_mem(adapter: &PciAdapter) -> Result<Vec<VirtioPciBar>, PciError> {
        let mut mapped_bars: Vec<VirtioPciBar> = Vec::new();

        for bar in &adapter.base_addresses {
            match bar {
                PciBar::IO(_) => {
			    	warn!("Cannot map I/O BAR!");
			    	continue;
			    },
			    PciBar::Memory(bar) => {
                    if bar.width != 64 {
                        warn!("Currently only mapping of 64 bit BAR's is supported!");
                        continue;
                    }
                    if !bar.prefetchable {
                        warn!("Currently only mapping of prefetchable BAR's is supported!");
                        continue;
                    }
                    
                    let virtual_address = VirtMemAddr::from(crate::mm::map(bar.addr, bar.size, true, true, true));
                    
                    mapped_bars.push(VirtioPciBar {
                        index: bar.index,
                        mem_addr: virtual_address,
                        // Unsafe cast of usize to u64
                        length: bar.size as u64,
                    })
                }
            } 
        }

        if mapped_bars.is_empty() {
            error!("No correct memory BAR for device {:x} found.", adapter.device_id);
            Err(PciError::NoBar(adapter.device_id))
        } else {
            Ok(mapped_bars)
        }
    }
}
