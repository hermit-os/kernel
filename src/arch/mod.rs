// Copyright (c) 2017 Stefan Lankes, RWTH Aachen University
//                    Colin Finck, RWTH Aachen University
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

// Platform-specific implementations
#[cfg(target_arch = "aarch64")]
pub mod aarch64;

#[cfg(target_arch = "x86_64")]
pub mod x86_64;

// Export our platform-specific modules.
#[cfg(target_arch = "aarch64")]
pub use arch::aarch64::*;

#[cfg(target_arch = "aarch64")]
pub use arch::aarch64::kernel::stubs::{set_oneshot_timer, switch, wakeup_core};

#[cfg(target_arch = "aarch64")]
pub use arch::aarch64::kernel::{
    application_processor_init, boot_application_processors, boot_processor_init,
    get_processor_count, message_output_init, output_message_byte,
};

#[cfg(target_arch = "aarch64")]
use arch::aarch64::kernel::percore::core_scheduler;

#[cfg(target_arch = "aarch64")]
pub use arch::aarch64::kernel::percore;

#[cfg(target_arch = "aarch64")]
pub use arch::aarch64::kernel::scheduler;

#[cfg(target_arch = "aarch64")]
pub use arch::aarch64::kernel::processor;

#[cfg(target_arch = "aarch64")]
pub use arch::aarch64::kernel::irq;

#[cfg(target_arch = "aarch64")]
pub use arch::aarch64::kernel::systemtime::get_boot_time;

#[cfg(target_arch = "x86_64")]
pub use arch::x86_64::*;

#[cfg(target_arch = "x86_64")]
pub use arch::x86_64::kernel::apic::{set_oneshot_timer, wakeup_core};
#[cfg(target_arch = "x86_64")]
pub use arch::x86_64::kernel::gdt::set_current_kernel_stack;
#[cfg(target_arch = "x86_64")]
pub use arch::x86_64::kernel::irq;
#[cfg(target_arch = "x86_64")]
pub use arch::x86_64::kernel::percore;
#[cfg(target_arch = "x86_64")]
pub use arch::x86_64::kernel::processor;
#[cfg(target_arch = "x86_64")]
pub use arch::x86_64::kernel::scheduler;
#[cfg(target_arch = "x86_64")]
pub use arch::x86_64::kernel::switch::switch;
#[cfg(target_arch = "x86_64")]
pub use arch::x86_64::kernel::systemtime::get_boot_time;
#[cfg(not(test))]
#[cfg(target_arch = "x86_64")]
pub use arch::x86_64::kernel::{
    application_processor_init, boot_application_processors, boot_processor_init,
};
#[cfg(target_arch = "x86_64")]
pub use arch::x86_64::kernel::{get_processor_count, message_output_init, output_message_byte};
