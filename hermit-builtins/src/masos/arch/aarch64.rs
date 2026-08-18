use core::arch::asm;

#[inline]
pub(crate) unsafe fn syscall0(nr: usize) -> usize {
	let r0;
	unsafe {
		asm!(
			"svc 0",
			in("x8") nr,
			lateout("x0") r0,
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
			"svc 0",
			in("x8") nr,
			inlateout("x0") a0 => r0,
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
			"svc 0",
			in("x8") nr,
			inlateout("x0") a0 => r0,
			in("x1") a1,
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
			"svc 0",
			in("x8") nr,
			inlateout("x0") a0 => r0,
			in("x1") a1,
			in("x2") a2,
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
			"svc 0",
			in("x8") nr,
			inlateout("x0") a0 => r0,
			in("x1") a1,
			in("x2") a2,
			in("x3") a3,
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
			"svc 0",
			in("x8") nr,
			inlateout("x0") a0 => r0,
			in("x1") a1,
			in("x2") a2,
			in("x3") a3,
			in("x4") a4,
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
			"svc 0",
			in("x8") nr,
			inlateout("x0") a0 => r0,
			in("x1") a1,
			in("x2") a2,
			in("x3") a3,
			in("x4") a4,
			in("x5") a5,
			options(preserves_flags),
		);
	}
	r0
}
