// Copyright (c) 2020 Frederik Schulz, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

//! A module containing virtios transport mechanisms.
//!
//! The module contains only PCI specifc transport mechanism.
//! Other mechanisms (MMIO and Channel I/O) are currently not
//! supported.

pub mod pci;
