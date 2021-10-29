use crate::syscalls::interfaces::SyscallInterface;

// The generic interface simply uses all default implementations of the
// SyscallInterface trait.
pub struct Generic;
impl SyscallInterface for Generic {}
