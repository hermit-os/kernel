use core::arch::asm;

#[inline]
pub(crate) unsafe fn syscall0(nr: usize) -> usize {
	let r0;
	unsafe {
		asm!(
			"syscall",
			inlateout("rax") nr => r0,
			lateout("rcx") _,
			lateout("r11") _,
			options(preserves_flags),
		);
	}
	r0
}

#[inline]
pub(crate) unsafe fn syscall1(nr: usize, a0: usize) -> usize {
	let r0;
	unsafe {
		asm!(
			"syscall",
			inlateout("rax") nr => r0,
			in("rdi") a0,
			lateout("rcx") _,
			lateout("r11") _,
			options(preserves_flags),
		);
	}
	r0
}

#[inline]
pub(crate) unsafe fn syscall2(nr: usize, a0: usize, a1: usize) -> usize {
	let r0;
	unsafe {
		asm!(
			"syscall",
			inlateout("rax") nr => r0,
			in("rdi") a0,
			in("rsi") a1,
			lateout("rcx") _,
			lateout("r11") _,
			options(preserves_flags),
		);
	}
	r0
}

#[inline]
pub(crate) unsafe fn syscall3(nr: usize, a0: usize, a1: usize, a2: usize) -> usize {
	let r0;
	unsafe {
		asm!(
			"syscall",
			inlateout("rax") nr => r0,
			in("rdi") a0,
			in("rsi") a1,
			in("rdx") a2,
			lateout("rcx") _,
			lateout("r11") _,
			options(preserves_flags),
		);
	}
	r0
}

#[inline]
pub(crate) unsafe fn syscall4(nr: usize, a0: usize, a1: usize, a2: usize, a3: usize) -> usize {
	let r0;
	unsafe {
		asm!(
			"syscall",
			inlateout("rax") nr => r0,
			in("rdi") a0,
			in("rsi") a1,
			in("rdx") a2,
			in("r10") a3,
			lateout("rcx") _,
			lateout("r11") _,
			options(preserves_flags),
		);
	}
	r0
}

#[inline]
pub(crate) unsafe fn syscall5(nr: usize, a0: usize, a1: usize, a2: usize, a3: usize, a4: usize) -> usize {
	let r0;
	unsafe {
		asm!(
			"syscall",
			inlateout("rax") nr =>r0,
			in("rdi") a0,
			in("rsi") a1,
			in("rdx") a2,
			in("r10") a3,
			in("r8") a4,
			lateout("rcx") _,
			lateout("r11") _,
			options(preserves_flags),
		);
	}
	r0
}

#[inline]
pub(crate) unsafe fn syscall6(
	nr: usize,
	a0: usize,
	a1: usize,
	a2: usize,
	a3: usize,
	a4: usize,
	a5: usize,
) -> usize {
	let r0;
	unsafe {
		asm!(
			"syscall",
			inlateout("rax") nr => r0,
			in("rdi") a0,
			in("rsi") a1,
			in("rdx") a2,
			in("r10") a3,
			in("r8") a4,
			in("r9") a5,
			lateout("rcx") _,
			lateout("r11") _,
			options(preserves_flags),
		);
	}
	r0
}
