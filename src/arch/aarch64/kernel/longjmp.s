# The code is derived from the musl implementation
# of longjmp.
.section .text
.global longjmp
longjmp:
	# IHI0055B_aapcs64.pdf 5.1.1, 5.1.2 callee saved registers
	ldp x19, x20, [x0,#0]
	ldp x21, x22, [x0,#16]
	ldp x23, x24, [x0,#32]
	ldp x25, x26, [x0,#48]
	ldp x27, x28, [x0,#64]
	ldp x29, x30, [x0,#80]
	ldr x2, [x0,#104]
	mov sp, x2

	cmp w1, 0
	csinc w0, w1, wzr, ne
	br x30