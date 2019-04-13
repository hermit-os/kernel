#[allow(dead_code)]
pub const COMMIT_HASH: &'static str = env!("GIT_HASH");

#[allow(dead_code)]
pub const KERNEL_STACK_SIZE: usize = 0x8000;

#[allow(dead_code)]
pub const DEFAULT_STACK_SIZE: usize = 0x40000;
