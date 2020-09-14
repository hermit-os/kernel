// Copyright (c) 2018 Stefan Lankes, RWTH Aachen University
//                    Colin Finck, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

use crate::syscalls::interfaces::SyscallInterface;

// The generic interface simply uses all default implementations of the
// SyscallInterface trait.
pub struct Generic;
impl SyscallInterface for Generic {}
