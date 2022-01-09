use crate::arch::aarch64::kernel::serial::SerialPort;
use crate::arch::aarch64::kernel::{
	get_processor_count, scheduler::TaskStacks, BootInfo, BOOT_INFO,
};
use crate::KERNEL_STACK_SIZE;

extern "C" {
	static vector_table: u8;
}

/*
 * Memory types available.
 */
#[allow(non_upper_case_globals)]
const MT_DEVICE_nGnRnE: u64 = 0;
#[allow(non_upper_case_globals)]
const MT_DEVICE_nGnRE: u64 = 1;
const MT_DEVICE_GRE: u64 = 2;
const MT_NORMAL_NC: u64 = 3;
const MT_NORMAL: u64 = 4;

fn mair(attr: u64, mt: u64) -> u64 {
	attr << (mt * 8)
}

/// Entrypoint - Initialize Stack pointer and Exception Table
#[no_mangle]
#[naked]
pub unsafe extern "C" fn _start() -> ! {
	asm!(
		/* determine stack address */
		"mov x1, x0",
		"add x1, x1, {current_stack_address_offset}",
		"ldr x2, [x1]",   /* Previous version subtracted 0x10 from End, so I'm doing this too. Not sure why though. COMMENT from SL: This is a habit of mine. I always start 0x10 bytes before the end of the stack. */
		"mov x3, {stack_top_offset}",
		"add x2, x2, x3",
		"mov sp, x2",
		"adrp x4, {pre_init}",
		"add  x4, x4, #:lo12:{pre_init}",
		"br x4",
		current_stack_address_offset = const BootInfo::current_stack_address_offset(),
		stack_top_offset = const KERNEL_STACK_SIZE - TaskStacks::MARKER_SIZE,
		pre_init = sym pre_init,
		options(noreturn),
	)
}

#[inline(never)]
#[no_mangle]
unsafe fn pre_init(boot_info: &'static mut BootInfo) -> ! {
	BOOT_INFO = boot_info as *mut BootInfo;

	/* disable interrupts */
	asm!(
		"msr daifset, {mask}",
		mask = const 0b111,
		options(nostack, nomem),
	);

	/* reset thread id registers */
	asm!(
		"msr tpidr_el0, xzr",
		"msr tpidr_el1, xzr",
		options(nostack, nomem),
	);

	/*
	 * Disable the MMU. We may have entered the kernel with it on and
	 * will need to update the tables later. If this has been set up
	 * with anything other than a VA == PA map then this will fail,
	 * but in this case the code to find where we are running from
	 * would have also failed.
	 */
	asm!(
		"dsb sy",
		"mrs x2, sctlr_el1",
		"bic x2, x2, {one}",
		"msr sctlr_el1, x2",
		"isb",
		one = const 0x1,
		out("x2") _,
		options(nostack, nomem),
	);

	asm!("ic iallu", "tlbi vmalle1is", "dsb ish", options(nostack),);

	/*
	 * Setup memory attribute type tables
	 *
	 * Memory regioin attributes for LPAE:
	 *
	 *   n = AttrIndx[2:0]
	 *                      n       MAIR
	 *   DEVICE_nGnRnE      000     00000000 (0x00)
	 *   DEVICE_nGnRE       001     00000100 (0x04)
	 *   DEVICE_GRE         010     00001100 (0x0c)
	 *   NORMAL_NC          011     01000100 (0x44)
	 *   NORMAL             100     11111111 (0xff)
	 */
	let mair_el1 = mair(0x00, MT_DEVICE_nGnRnE)
		| mair(0x04, MT_DEVICE_nGnRE)
		| mair(0x0c, MT_DEVICE_GRE)
		| mair(0x44, MT_NORMAL_NC)
		| mair(0xff, MT_NORMAL);
	asm!(
		"msr mair_el1, {0}",
		in(reg) mair_el1,
		options(nostack, nomem),
	);

	/*
	 * Enable FP/ASIMD in Architectural Feature Access Control Register,
	 */
	asm!(
		"msr cpacr_el1, {mask}",
		mask = in(reg) 3 << 20,
		options(nostack, nomem),
	);

	/*
	 * Reset debug control register
	 */
	asm!("msr mdscr_el1, xzr", options(nostack, nomem),);

	/* set exception table */
	asm!(
		"adrp x4, {vector_table}",
		"add  x4, x4, #:lo12:{vector_table}",
		"msr vbar_el1, x4",
		vector_table = sym vector_table,
		out("x4") _,
		options(nostack, nomem),
	);

	/* Memory barrier */
	asm!("dsb sy", options(nostack),);

	let sctrl_el1: u64 = 0
	 | (1 << 26) 	    /* UCI     	Enables EL0 access in AArch64 for DC CVAU, DC CIVAC,
				 				    DC CVAC and IC IVAU instructions */
	 | (0 << 25)		/* EE      	Explicit data accesses at EL1 and Stage 1 translation
	 			 				    table walks at EL1 & EL0 are little-endian */
	 | (0 << 24)		/* EOE     	Explicit data accesses at EL0 are little-endian */
	 | (1 << 23)
	 | (1 << 22)
	 | (1 << 20)
	 | (0 << 19)		/* WXN     	Regions with write permission are not forced to XN */
	 | (1 << 18)		/* nTWE     WFE instructions are executed as normal */
	 | (0 << 17)
	 | (1 << 16)		/* nTWI    	WFI instructions are executed as normal */
	 | (1 << 15)		/* UCT     	Enables EL0 access in AArch64 to the CTR_EL0 register */
	 | (1 << 14)		/* DZE     	Execution of the DC ZVA instruction is allowed at EL0 */
	 | (0 << 13)
	 | (1 << 12)		/* I       	Instruction caches enabled at EL0 and EL1 */
	 | (1 << 11)
	 | (0 << 10)
	 | (0 << 9)			/* UMA      Disable access to the interrupt masks from EL0 */
	 | (1 << 8)			/* SED      The SETEND instruction is available */
	 | (0 << 7)			/* ITD      The IT instruction functionality is available */
	 | (0 << 6)			/* THEE    	ThumbEE is disabled */
	 | (0 << 5)			/* CP15BEN  CP15 barrier operations disabled */
	 | (1 << 4)			/* SA0     	Stack Alignment check for EL0 enabled */
	 | (1 << 3)			/* SA      	Stack Alignment check enabled */
	 | (1 << 2)			/* C       	Data and unified enabled */
	 | (0 << 1)			/* A       	Alignment fault checking disabled */
	 | (0 << 0)			/* M       	MMU enable */
	;

	asm!(
		"msr sctlr_el1, {0}",
		in(reg) sctrl_el1,
		options(nostack),
	);

	if boot_info.cpu_online == 0 {
		crate::boot_processor_main()
	} else {
		#[cfg(not(feature = "smp"))]
		{
			error!("SMP support deactivated");
			loop {
				crate::arch::processor::halt()
			}
		}
		#[cfg(feature = "smp")]
		crate::application_processor_main();
	}
}
