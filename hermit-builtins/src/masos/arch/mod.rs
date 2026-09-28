cfg_select! {
	target_arch = "aarch64" => {
		mod aarch64;
		pub(crate) use aarch64::*;
	}
	target_arch = "x86_64" => {
		mod x86_64;
		pub(crate) use x86_64::*;
	}
	_ => {}
}

macro_rules! syscall {
	($nr:expr $(,)?) => {
		$crate::masos::arch::syscall0($nr as usize)
	};

	($nr:expr, $a0:expr $(,)?) => {
		$crate::masos::arch::syscall1($nr as usize, $a0 as usize)
	};

	($nr:expr, $a0:expr, $a1:expr $(,)?) => {
		$crate::masos::arch::syscall2($nr as usize, $a0 as usize, $a1 as usize)
	};

	($nr:expr, $a0:expr, $a1:expr, $a2:expr $(,)?) => {
		$crate::masos::arch::syscall3($nr as usize, $a0 as usize, $a1 as usize, $a2 as usize)
	};

	($nr:expr, $a0:expr, $a1:expr, $a2:expr, $a3:expr $(,)?) => {
		$crate::masos::arch::syscall4(
			$nr as usize,
			$a0 as usize,
			$a1 as usize,
			$a2 as usize,
			$a3 as usize,
		)
	};

	($nr:expr, $a0:expr, $a1:expr, $a2:expr, $a3:expr, $a4:expr $(,)?) => {
		$crate::masos::arch::syscall5(
			$nr as usize,
			$a0 as usize,
			$a1 as usize,
			$a2 as usize,
			$a3 as usize,
			$a4 as usize,
		)
	};

	($nr:expr, $a0:expr, $a1:expr, $a2:expr, $a3:expr, $a4:expr, $a5:expr $(,)?) => {
		$crate::masos::arch::syscall6(
			$nr as usize,
			$a0 as usize,
			$a1 as usize,
			$a2 as usize,
			$a3 as usize,
			$a4 as usize,
			$a5 as usize,
		)
	};
}
