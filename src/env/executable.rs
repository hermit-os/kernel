use core::ops::Range;

pub fn executable_ptr_range() -> Range<*mut ()> {
	executable_start()..executable_end()
}

fn executable_start() -> *mut () {
	unsafe extern "C" {
		/// Start of the executable.
		///
		/// The address of `__executable_start` is the location of the first
		/// loadable segment. Apart from changelogs, it is not documented, but
		/// defined by:
		///
		/// - ld: [binutils-gdb@`c6d3b05`]
		/// - gold: [binutils-gdb@`f6ce93d`]
		/// - lld: [llvm/llvm-project@`0e454a9`]
		/// - mold: [rui314/mold@`5f492fe`]
		/// - Wild: [wild-linker/wild@`0a21948`]
		///
		/// [binutils-gdb@`c6d3b05`]: https://sourceware.org/git/?p=binutils-gdb.git;a=commit;h=c6d3b05fe766fe33bb96b8850559c9ada7296dd4
		/// [binutils-gdb@`f6ce93d`]: https://sourceware.org/git/?p=binutils-gdb.git;a=commit;h=f6ce93d6e999d1a0c450c5e71c5b3468e6217f0a
		/// [llvm/llvm-project@`0e454a9`]: https://github.com/llvm/llvm-project/commit/0e454a9837c312807e8791dfcd8607cbc18d4359
		/// [rui314/mold@`5f492fe`]: https://github.com/rui314/mold/commit/5f492fea708029656ddaea8e9b53a8fc3b503b7a
		/// [wild-linker/wild@`0a21948`]: https://github.com/wild-linker/wild/commit/0a219486590a3349c803377170beed9afe759210
		static mut __executable_start: u8;
	}

	(&raw mut __executable_start).cast::<()>()
}

fn executable_end() -> *mut () {
	unsafe extern "C" {
		/// End of the executable.
		///
		/// The address of `_end` is the first location after the last loadable
		/// segment. For details, see [etext(3C)]. It is defined by:
		///
		/// - ld: [binutils-gdb@`252b513`]
		/// - gold: [binutils-gdb@`ead1e42`]
		/// - lld: [llvm/llvm-project@`b044af5`]
		/// - mold: [rui314/mold@`694ae9a`]
		/// - Wild: [wild-linker/wild@`fb7da78`]
		///
		/// [etext(3C)]: https://docs.oracle.com/cd/E86824_01/html/E54766/etext-3c.html
		/// [binutils-gdb@`252b513`]: https://sourceware.org/git/?p=binutils-gdb.git;a=commit;h=252b5132c753830d5fd56823373aed85f2a0db63
		/// [binutils-gdb@`ead1e42`]: https://sourceware.org/git/?p=binutils-gdb.git;a=commit;h=ead1e4244a55707685d105c662a9a1faf5d122fe
		/// [llvm/llvm-project@`b044af5`]: https://github.com/llvm/llvm-project/commit/b044af50f28209ff4eeed8fa4614e78969d8df74
		/// [rui314/mold@`694ae9a`]: https://github.com/rui314/mold/commit/694ae9a9c5282809db85dd9f3858e8f697989843
		/// [wild-linker/wild@`fb7da78`]: https://github.com/wild-linker/wild/commit/fb7da7841ad9e64e2cd1128a62d23d488dae921d
		static mut _end: u8;
	}

	(&raw mut _end).cast::<()>()
}
