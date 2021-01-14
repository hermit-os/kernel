// Copyright (c) 2017 Stefan Lankes, RWTH Aachen University
//                    Colin Finck, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

// Platform-specific implementations
#[cfg(target_arch = "aarch64")]
pub mod aarch64;

#[cfg(target_arch = "x86_64")]
pub mod x86_64;

// Export our platform-specific modules.
#[cfg(target_arch = "aarch64")]
pub use crate::arch::aarch64::*;

#[cfg(target_arch = "aarch64")]
pub use crate::arch::aarch64::kernel::stubs::{set_oneshot_timer, switch, wakeup_core};

#[cfg(target_arch = "aarch64")]
pub use crate::arch::aarch64::kernel::{
	application_processor_init, boot_application_processors, boot_processor_init,
	get_processor_count, message_output_init, output_message_byte,
};

#[cfg(target_arch = "aarch64")]
use crate::arch::aarch64::kernel::percore::core_scheduler;

#[cfg(target_arch = "aarch64")]
pub use crate::arch::aarch64::kernel::percore;

#[cfg(target_arch = "aarch64")]
pub use crate::arch::aarch64::kernel::scheduler;

#[cfg(target_arch = "aarch64")]
pub use crate::arch::aarch64::kernel::processor;

#[cfg(target_arch = "aarch64")]
pub use crate::arch::aarch64::kernel::irq;

#[cfg(target_arch = "aarch64")]
pub use crate::arch::aarch64::kernel::systemtime::get_boot_time;

#[cfg(target_arch = "x86_64")]
pub use crate::arch::x86_64::*;

#[cfg(target_arch = "x86_64")]
pub use crate::arch::x86_64::kernel::apic::{set_oneshot_timer, wakeup_core};
#[cfg(all(target_arch = "x86_64", target_os = "hermit", feature = "smp"))]
pub use crate::arch::x86_64::kernel::application_processor_init;
#[cfg(target_arch = "x86_64")]
pub use crate::arch::x86_64::kernel::gdt::set_current_kernel_stack;
#[cfg(target_arch = "x86_64")]
pub use crate::arch::x86_64::kernel::irq;
#[cfg(target_arch = "x86_64")]
pub use crate::arch::x86_64::kernel::percore;
#[cfg(target_arch = "x86_64")]
pub use crate::arch::x86_64::kernel::processor;
#[cfg(target_arch = "x86_64")]
pub use crate::arch::x86_64::kernel::scheduler;
#[cfg(target_arch = "x86_64")]
pub use crate::arch::x86_64::kernel::systemtime::get_boot_time;
#[cfg(all(target_arch = "x86_64", target_os = "hermit"))]
pub use crate::arch::x86_64::kernel::{boot_application_processors, boot_processor_init};
#[cfg(target_arch = "x86_64")]
pub use crate::arch::x86_64::kernel::{
	get_processor_count, message_output_init, output_message_buf, output_message_byte,
};

#[cfg(test)]
pub fn switch_to_task(_old_stack: *mut usize, _new_stack: usize) {}
#[cfg(test)]
pub fn switch_to_fpu_owner(_old_stack: *mut usize, _new_stack: usize) {}

#[cfg(not(test))]
extern "C" {
	pub fn switch_to_task(old_stack: *mut usize, new_stack: usize);
	pub fn switch_to_fpu_owner(old_stack: *mut usize, new_stack: usize);
}
