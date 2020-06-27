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

#[derive(Copy, Clone, Debug)]
pub struct VirtMemAddr(usize);

impl From<u32> for VirtMemAddr {
    fn from(addr: u32) -> Self {
        unimplemented!();
        // TODO: check if current system is 32 bit, then okay. else fail
    }
}

impl From<u64> for VirtMemAddr {
    fn from(addr: u64) -> Self {
        unimplemented!();
        // TODO: check if current system is 64 bit, then okaym ekse fail
    }
}

impl From<usize> for VirtMemAddr {
    fn from (addr: usize) -> Self {
        VirtMemAddr(addr)
    }
}

pub struct PhyMemAddr(usize);

impl From<u32> for PhyMemAddr {
    fn from(addr: u32) -> Self {
        unimplemented!();
        // TODO: check if current system is 32 bit, then okay. else fail
    }
}

impl From<u64> for PhyMemAddr {
    fn from(addr: u64) -> Self {
        unimplemented!();
        // TODO: check if current system is 64 bit, then okaym ekse fail
    }
}

impl From<usize> for PhyMemAddr {
    fn from(addr: usize) -> Self {
        PhyMemAddr(addr)
    }
}

pub mod pci {
    use drivers::virtio::env::{VirtMemAddr, PhyMemAddr};
    use drivers::virtio::transport::pci::PciBar as VirtioPciBar;
    use drivers::virtio::types::Le32;
    use arch::x86_64::kernel::pci;
    use arch::x86_64::kernel::pci::{PciAdapter, PciBar, IOBar, MemoryBar};
    use arch::x86_64::kernel::pci::error::PciError;
    use alloc::vec::Vec;
    use core::result::Result;

    /// Wrapper function to read the configuration space of a PCI 
    /// device at the given register. Returns the registers value.
    ///
    pub fn read_config(adapter: &PciAdapter, register: u32) -> u32 {
        // Takes care of converting to targets endianess.
        u32::from_le(pci::read_config(adapter.bus, adapter.device, register.to_le()))
    }

    /// Wrapper function to write the configuraiton space of a PCI
    /// device at the given register.
    pub fn write_config(adapter: &PciAdapter, register: u32, data: u32) {
        pci::write_config(adapter.bus, adapter.device, register.to_le(), data.to_le());
    }


    /// Maps all memeory areas indicated by the devices BAR's into 
    /// Virtual address space. 
    ///
    /// As this function uses parts of the kernel pci code it is 
    /// outsourced into the env::pci module.
    pub fn map_bar_mem(adapter: &PciAdapter) -> Result<Vec<VirtioPciBar>, PciError> {
        let mut mapped_bars: Vec<VirtioPciBar> = Vec::new();

        for bar in &adapter.base_addresses {
            match bar {
                PciBar::IO(_) => {
			    	warn!("Cannot map IOBar!");
			    	continue;
			    },
			    PciBar::Memory(bar) => {
                    if bar.width != 64 {
                        warn!("Currently only mapping of 64 bit bars is supported!");
                        continue;
                    }
                    if !bar.prefetchable {
                        warn!("Currently only mapping of prefetchable bars is supported!");
                        continue;
                    }
                    
                    let virtual_address = VirtMemAddr::from(crate::mm::map(bar.addr, bar.size, true, true, true));
                    
                    mapped_bars.push(VirtioPciBar {
                        index: bar.index,
                        mem_addr: virtual_address,
                        length: bar.size,
                    })
                }
            } 
        }

        if mapped_bars.is_empty() {
            Err(PciError::NoBar(adapter.device_id))
        } else {
            Ok(mapped_bars)
        }
    }
}
