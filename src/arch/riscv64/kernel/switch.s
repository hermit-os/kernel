.section .text
.global switch_to_task
.global task_start
.extern task_entry
// .extern set_current_kernel_stack


.align 16
// This function should only be called if the fp registers are clean
switch_to_task:
	// a0 = old_stack => the address to store the old rsp
	// a1 = new_stack => stack pointer of the new task
	
	addi sp, sp, -(31*8)
	sd x31, (8*30)(sp)
	sd x30, (8*29)(sp)
	sd x29, (8*28)(sp)
	sd x28, (8*27)(sp)
	sd x27, (8*26)(sp)
	sd x26, (8*25)(sp)
	sd x25, (8*24)(sp)
	sd x24, (8*23)(sp)
	sd x23, (8*22)(sp)
	sd x22, (8*21)(sp)
	sd x21, (8*20)(sp)
	sd x20, (8*19)(sp)
	sd x19, (8*18)(sp)
	sd x18, (8*17)(sp)
	sd x17, (8*16)(sp)
	sd x16, (8*15)(sp)
	sd x15, (8*14)(sp)
	sd x14, (8*13)(sp)
	sd x13, (8*12)(sp)
	sd x12, (8*11)(sp)
	sd x11, (8*10)(sp)
	sd x10, (8*9)(sp)
	sd x9, (8*8)(sp)
	sd x8, (8*7)(sp)
	sd x7, (8*6)(sp)
	sd x6, (8*5)(sp)
	sd x5, (8*4)(sp)
	sd x4, (8*3)(sp)
	//sd x3, (8*2)(sp)
	//sd x2, (8*1)(sp)
	sd x1, (8*0)(sp)

	//Store floating point registers 
	//TODO: Save only when changed
	# fsd f0, (8*31)(sp)
	# fsd f1, (8*32)(sp)
	# fsd f2, (8*33)(sp)
	# fsd f3, (8*34)(sp)
	# fsd f4, (8*35)(sp)
	# fsd f5, (8*36)(sp)
	# fsd f6, (8*37)(sp)
	# fsd f7, (8*38)(sp)
	# fsd f8, (8*39)(sp)
	# fsd f9, (8*40)(sp)
	# fsd f10, (8*41)(sp)
	# fsd f11, (8*42)(sp)
	# fsd f12, (8*43)(sp)
	# fsd f13, (8*44)(sp)
	# fsd f14, (8*45)(sp)
	# fsd f15, (8*46)(sp)
	# fsd f16, (8*47)(sp)
	# fsd f17, (8*48)(sp)
	# fsd f18, (8*49)(sp)
	# fsd f19, (8*50)(sp)
	# fsd f20, (8*51)(sp)
	# fsd f21, (8*52)(sp)
	# fsd f22, (8*53)(sp)
	# fsd f23, (8*54)(sp)
	# fsd f24, (8*55)(sp)
	# fsd f25, (8*56)(sp)
	# fsd f26, (8*57)(sp)
	# fsd f27, (8*58)(sp)
	# fsd f28, (8*59)(sp)
	# fsd f29, (8*60)(sp)
	# fsd f30, (8*61)(sp)
	# fsd f31, (8*62)(sp)
	# frcsr t0
	# sd t0, (8*63)(sp)

    // Store current stack pointer with saved context in `_dst`.
	sd sp, (0)(a0)
	// Set stack pointer to supplied `_src`.
	mv sp, a1

	//set current kernel stack
	call set_current_kernel_stack

	# // Restore fp regs
	# fld f0, (8*31)(sp)
	# fld f1, (8*32)(sp)
	# fld f2, (8*33)(sp)
	# fld f3, (8*34)(sp)
	# fld f4, (8*35)(sp)
	# fld f5, (8*36)(sp)
	# fld f6, (8*37)(sp)
	# fld f7, (8*38)(sp)
	# fld f8, (8*39)(sp)
	# fld f9, (8*40)(sp)
	# fld f10, (8*41)(sp)
	# fld f11, (8*42)(sp)
	# fld f12, (8*43)(sp)
	# fld f13, (8*44)(sp)
	# fld f14, (8*45)(sp)
	# fld f15, (8*46)(sp)
	# fld f16, (8*47)(sp)
	# fld f17, (8*48)(sp)
	# fld f18, (8*49)(sp)
	# fld f19, (8*50)(sp)
	# fld f20, (8*51)(sp)
	# fld f21, (8*52)(sp)
	# fld f22, (8*53)(sp)
	# fld f23, (8*54)(sp)
	# fld f24, (8*55)(sp)
	# fld f25, (8*56)(sp)
	# fld f26, (8*57)(sp)
	# fld f27, (8*58)(sp)
	# fld f28, (8*59)(sp)
	# fld f29, (8*60)(sp)
	# fld f30, (8*61)(sp)
	# fld f31, (8*62)(sp)
	# ld t0, (8*63)(sp)
	# fscsr t0

	// Restore context
	ld x1, (8*0)(sp)
	//ld x2, (8*1)(sp)
	//ld x3, (8*2)(sp)
	ld x4, (8*3)(sp)
	ld x5, (8*4)(sp)
	ld x6, (8*5)(sp)
	ld x7, (8*6)(sp)
	ld x8, (8*7)(sp)
	ld x9, (8*8)(sp)
	ld x10, (8*9)(sp)
	ld x11, (8*10)(sp)
	ld x12, (8*11)(sp)
	ld x13, (8*12)(sp)
	ld x14, (8*13)(sp)
	ld x15, (8*14)(sp)
	ld x16, (8*15)(sp)
	ld x17, (8*16)(sp)
	ld x18, (8*17)(sp)
	ld x19, (8*18)(sp)
	ld x20, (8*19)(sp)
	ld x21, (8*20)(sp)
	ld x22, (8*21)(sp)
	ld x23, (8*22)(sp)
	ld x24, (8*23)(sp)
	ld x25, (8*24)(sp)
	ld x26, (8*25)(sp)
	ld x27, (8*26)(sp)
	ld x28, (8*27)(sp)
	ld x29, (8*28)(sp)
	ld x30, (8*29)(sp)
	ld x31, (8*30)(sp)

	addi sp, sp, (31*8)

	ret

.align 16
task_start:
	mv sp, a2
	j task_entry
