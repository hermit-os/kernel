# The code is derived from the musl implementation
# of longjmp.
.section .text
.global longjmp
longjmp:
    ld s0,    0(a0)
	ld s1,    8(a0)
	ld s2,    16(a0)
	ld s3,    24(a0)
	ld s4,    32(a0)
	ld s5,    40(a0)
	ld s6,    48(a0)
	ld s7,    56(a0)
	ld s8,    64(a0)
	ld s9,    72(a0)
	ld s10,   80(a0)
	ld s11,   88(a0)
	ld sp,    96(a0)
	ld ra,    104(a0)

	seqz a0, a1
	add a0, a0, a1
	ret