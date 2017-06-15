/*
 * Copyright (c) 2010-2015, Stefan Lankes, RWTH Aachen University
 * All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions are met:
 *    * Redistributions of source code must retain the above copyright
 *      notice, this list of conditions and the following disclaimer.
 *    * Redistributions in binary form must reproduce the above copyright
 *      notice, this list of conditions and the following disclaimer in the
 *      documentation and/or other materials provided with the distribution.
 *    * Neither the name of the University nor the names of its contributors
 *      may be used to endorse or promote products derived from this
 *      software without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED
 * WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
 * DISCLAIMED. IN NO EVENT SHALL THE REGENTS OR CONTRIBUTORS BE LIABLE FOR ANY
 * DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES
 * (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES;
 * LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND
 * ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
 * (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS
 * SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 */

#include <hermit/stddef.h>
#include <hermit/stdio.h>
#include <hermit/stdlib.h>
#include <hermit/string.h>
#include <hermit/errno.h>
#include <hermit/processor.h>
#include <hermit/time.h>
#include <hermit/spinlock.h>
#include <hermit/vma.h>
#include <hermit/tasks.h>
#include <hermit/logging.h>
#include <asm/irq.h>
#include <asm/idt.h>
#include <asm/irqflags.h>
#include <asm/io.h>
#include <asm/page.h>
#include <asm/apic.h>
#include <hermit/boot.h>

/*
 * Note that linker symbols are not variables, they have no memory allocated for
 * maintaining a value, rather their address is their value.
 */
extern const void kernel_start;

#define IOAPIC_ADDR	((size_t) &kernel_start - 2*PAGE_SIZE)
#define LAPIC_ADDR	((size_t) &kernel_start - 1*PAGE_SIZE)
#define MAX_APIC_CORES	MAX_CORES
#define SMP_SETUP_ADDR	0x8000ULL

// IO APIC MMIO structure: write reg, then read or write data.
typedef struct {
	uint32_t reg;
	uint32_t pad[3];
	uint32_t data;
} ioapic_t;

static const apic_processor_entry_t* apic_processors[MAX_APIC_CORES] = {[0 ... MAX_APIC_CORES-1] = NULL};
extern int32_t boot_processor;
extern uint32_t cpu_freq;
extern atomic_int32_t cpu_online;
extern int32_t isle;
extern int32_t possible_isles;
extern int32_t possible_cpus;
extern atomic_int32_t current_boot_id;
apic_mp_t* apic_mp  __attribute__ ((section (".data"))) = NULL;
static apic_config_table_t* apic_config = NULL;
static size_t lapic = 0;
static volatile ioapic_t* ioapic = NULL;
static uint32_t icr = 0;
static uint32_t ncores = 1;
static uint8_t irq_redirect[16] = { 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 0xA, 0xB, 0xC, 0xD, 0xE, 0xF};
static uint8_t apic_initialized = 0;
static uint8_t online[MAX_APIC_CORES] = {[0 ... MAX_APIC_CORES-1] = 0};

/*
 * The Multiprocessor Specification 1.4 (1997) suggests a 10ms delay
 * between the BSP asserting INIT and de-asserting INIT, when starting
 * a processor. But that slows the boot time off modern processors,
 * which include many cores and don't require that delay.
 *
 * => we use per default a lower delay to improve the boot time
 * => by setting traditional_delay to 1, we switch back to the old
 *    way
 */
#define traditional_delay 0

spinlock_t bootlock = SPINLOCK_INIT;

// forward declaration
static int lapic_reset(void);

static uint32_t lapic_read_default(uint32_t addr)
{
	return *((const volatile uint32_t*) (lapic+addr));
}

static uint32_t lapic_read_msr(uint32_t addr)
{
	return rdmsr(0x800 + (addr >> 4));
}

typedef uint32_t (*lapic_read_func)(uint32_t addr);

static lapic_read_func lapic_read = lapic_read_default;

static void lapic_write_default(uint32_t addr, uint32_t value)
{
#if 0
	/*
	 * to avoid a pentium bug, we have to read a apic register
	 * before we write a value to this register
	 */
	asm volatile ("movl (%%eax), %%edx; movl %%ebx, (%%eax)" :: "a"(lapic+addr), "b"(value) : "%edx");
#else
	*((volatile uint32_t*) (lapic+addr)) = value;
#endif
}

static void lapic_write_msr(uint32_t addr, uint32_t value)
{
	wrmsr(0x800 + (addr >> 4), value);
}

typedef void (*lapic_write_func)(uint32_t addr, uint32_t value);

static lapic_write_func lapic_write = lapic_write_default;

static inline uint32_t ioapic_read(uint32_t reg)
{
	ioapic->reg = reg;

	return ioapic->data;
}

static inline void ioapic_write(uint32_t reg, uint32_t value)
{
	ioapic->reg = reg;
	ioapic->data = value;
}

static inline uint32_t ioapic_version(void)
{
	if (ioapic)
		return ioapic_read(IOAPIC_REG_VER) & 0xFF;

	return 0;
}

static inline uint8_t ioapic_max_redirection_entry(void)
{
	if (ioapic)
		 return (ioapic_read(IOAPIC_REG_VER) >> 16) & 0xFF;

	return 0;
}

int apic_is_enabled(void)
{
	return (lapic && apic_initialized);
}

static inline void lapic_timer_set_counter(uint32_t counter)
{
	// set counter decrements to 1
	lapic_write(APIC_DCR, 0xB);
	lapic_write(APIC_ICR, counter);
}

static inline void lapic_timer_disable(void)
{
	lapic_write(APIC_LVT_TSR, 0x10000);
}

static inline void lapic_timer_oneshot(void)
{
	lapic_write(APIC_LVT_T, 0x7B);
}

static inline void lapic_timer_periodic(void)
{
	lapic_write(APIC_LVT_T, 0x2007B);
}

extern uint32_t disable_x2apic;

static inline void x2apic_disable(void)
{
	uint64_t msr;

	if (!has_x2apic())
		return;
	if (!disable_x2apic)
		return;

	msr = rdmsr(MSR_APIC_BASE);
	if (!(msr & MSR_X2APIC_ENABLE)) {
		LOG_WARNING("X2APIC already disabled!\n");
		return;
	}

	/* Disable xapic and x2apic first and then reenable xapic mode */
	wrmsr(MSR_APIC_BASE, msr & ~(MSR_X2APIC_ENABLE | MSR_XAPIC_ENABLE));
	wrmsr(MSR_APIC_BASE, msr & ~MSR_X2APIC_ENABLE);

	LOG_DEBUG("Disable X2APIC support\n");
	lapic_read = lapic_read_default;
	lapic_write = lapic_write_default;
}

static inline void x2apic_enable(void)
{
	uint64_t msr;

	if (!has_x2apic())
		return;

	if (lapic_read != lapic_read_msr)
		lapic_read = lapic_read_msr;
	if (lapic_write != lapic_write_msr)
		lapic_write = lapic_write_msr;

	msr = rdmsr(MSR_APIC_BASE);
	if (msr & MSR_X2APIC_ENABLE) {
		LOG_WARNING("X2APIC already enabled!\n");
                return;
	}

	wrmsr(MSR_APIC_BASE, msr | MSR_X2APIC_ENABLE);

	LOG_DEBUG("Enable X2APIC support!\n");
}

/*
 * Send a 'End of Interrupt' command to the APIC
 */
void apic_eoi(size_t int_no)
{
	/*
	 * If the IDT entry that was invoked was greater-than-or-equal to 48,
	 * then we use the APIC
	 */
	if (apic_is_enabled() || int_no >= 123) {
		lapic_write(APIC_EOI, 0);
	} else {
		/*
		 * If the IDT entry that was invoked was greater-than-or-equal to 40
		 * and lower than 48 (meaning IRQ8 - 15), then we need to
		 * send an EOI to the slave controller of the PIC
		 */
		if (int_no >= 40)
			outportb(0xA0, 0x20);

		/*
		 * In either case, we need to send an EOI to the master
		 * interrupt controller of the PIC, too
		 */
		outportb(0x20, 0x20);
	}
}

uint32_t apic_cpu_id(void)
{
	int32_t id = -1;

	if (apic_is_enabled())
		id = lapic_read(APIC_ID);

	if ((id >= 0) && has_x2apic())
		return id;
	else if (id >= 0)
		return (id >> 24);
	else if (boot_processor >= 0)
		return boot_processor;
	else
		return 0;
}

static inline uint32_t apic_version(void)
{
	if (lapic)
		return lapic_read(APIC_VERSION) & 0xFF;

	return 0;
}

static inline uint32_t apic_broadcast(void)
{
	if (lapic)
		return lapic_read(APIC_VERSION) & (1 << 24);

	return 0;
}

static inline uint32_t apic_lvt_entries(void)
{
	if (lapic)
		return (lapic_read(APIC_VERSION) >> 16) & 0xFF;

	return 0;
}

static inline void set_ipi_dest(uint32_t cpu_id) {
	uint32_t tmp;

	tmp = lapic_read(APIC_ICR2);
	tmp &= 0x00FFFFFF;
	tmp |= (cpu_id << 24);
	lapic_write(APIC_ICR2, tmp);
}

int apic_timer_is_running(void)
{
	if (BUILTIN_EXPECT(apic_is_enabled(), 1)) {
		return lapic_read(APIC_CCR) != 0;
	}

	return 0;
}

int apic_timer_deadline(uint32_t ticks)
{
	if (BUILTIN_EXPECT(apic_is_enabled() && icr, 1)) {
		LOG_DEBUG("timer oneshot %ld at core %d\n", ticks, CORE_ID);
		lapic_timer_oneshot();
		lapic_timer_set_counter(ticks * icr);

		return 0;
	}

	return -EINVAL;
}

int apic_disable_timer(void)
{
	if (BUILTIN_EXPECT(!apic_is_enabled(), 0))
		return -EINVAL;

	//kprintf("Disable local APIC timer at core %d\n", CORE_ID);
	lapic_timer_disable();

	return 0;
}

int apic_enable_timer(void)
{
	if (BUILTIN_EXPECT(apic_is_enabled() && icr, 1)) {
		//kprintf("Enable local APIC timer at core %d\n", CORE_ID);

		lapic_timer_periodic();
		lapic_timer_set_counter(icr);

		return 0;
	}

	return -EINVAL;
}

static apic_mp_t* search_mptable(size_t base, size_t limit) {
	size_t ptr=PAGE_CEIL(base), vptr=0;
	size_t flags = PG_GLOBAL | PG_RW | PG_PCD;
	apic_mp_t* tmp;
	uint32_t i;

	// protec apic by the NX flags
	if (has_nx())
		flags |= PG_XD;

	while(ptr<=limit-sizeof(apic_mp_t)) {
		if (vptr) {
			// unmap page via mapping a zero page
			page_unmap(vptr, 1);
			vptr = 0;
		}

		if (BUILTIN_EXPECT(!page_map(ptr & PAGE_MASK, ptr & PAGE_MASK, 1, flags), 1)) {
			vptr = ptr & PAGE_MASK;
		} else {
			kprintf("Failed to map 0x%zx, which is required to search for the MP tables\n", ptr);
			return NULL;
		}

		for(i=0; (vptr) && (i<PAGE_SIZE); i+=4) {
			tmp = (apic_mp_t*) (vptr+i);
			if (tmp->signature == MP_FLT_SIGNATURE) {
				if (!((tmp->version > 4) || (tmp->features[0]))) {
					vma_add(ptr & PAGE_MASK, (ptr & PAGE_MASK) + PAGE_SIZE, VMA_READ|VMA_WRITE);
					return tmp;
				}
			}
		}

		ptr += PAGE_SIZE;
	}

	if (vptr) {
		// unmap page via mapping a zero page
		page_unmap(vptr, 1);
	}

	return NULL;
}

#if 0
static size_t search_ebda(void) {
	size_t ptr=PAGE_CEIL(0x400), vptr=0xF0000;
	size_t flags = PG_GLOBAL | PG_RW | PG_PCD;

	// protec apic by the NX flags
	if (has_nx())
		flags |= PG_XD;

	if (BUILTIN_EXPECT(page_map(vptr, ptr & PAGE_MASK, 1, flags), 0))
		return 0;

	uint16_t addr = *((uint16_t*) (vptr+0x40E));
	LOG_INFO("Found EBDA at 0x%x!\n", (uint32_t)addr);

	// unmap page via mapping a zero page
	page_unmap(vptr, 1);

	return (size_t) addr;
}
#endif

static int lapic_reset(void)
{
	uint32_t max_lvt;

	if (!lapic)
		return -ENXIO;

	//x2apic_enable();

	max_lvt = apic_lvt_entries();

	lapic_write(APIC_SVR, 0x17F);	// enable the apic and connect to the idt entry 127
	lapic_write(APIC_TPR, 0x00);	// allow all interrupts
#ifdef DYNAMIC_TICKS
	lapic_timer_disable();
#else
	if (icr) {
		lapic_timer_periodic();
		lapic_timer_set_counter(icr);
	} else
		lapic_timer_disable();
#endif
	if (max_lvt >= 4)
		lapic_write(APIC_LVT_TSR, 0x10000);	// disable thermal sensor interrupt
	if (max_lvt >= 5)
		lapic_write(APIC_LVT_PMC, 0x10000);	// disable performance counter interrupt
	lapic_write(APIC_LINT0, 0x00010000);	// disable LINT0
	lapic_write(APIC_LINT1, 0x00010000);	// disable LINT1
	lapic_write(APIC_LVT_ER, 0x7E);	// connect error to idt entry 126

	return 0;
}

#if MAX_CORES > 1
/*
 * use the universal startup algorithm of Intel's MultiProcessor Specification
 */
static int wakeup_ap(uint32_t start_eip, uint32_t id)
{
	static char* reset_vector = 0;
	uint32_t i;

	LOG_INFO("Wakeup application processor %d via IPI\n", id);

	// set shutdown code to 0x0A
	cmos_write(0x0F, 0x0A);

	if (!reset_vector) {
		reset_vector = (char*) vma_alloc(PAGE_SIZE, VMA_READ|VMA_WRITE);
		page_map((size_t)reset_vector, 0x00, 1, PG_RW|PG_GLOBAL|PG_PCD);
		reset_vector += 0x467; // add base address of the reset vector
		LOG_DEBUG("Map reset vector to %p\n", reset_vector);
	}

	*((volatile unsigned short *) (reset_vector+2)) = start_eip >> 4;
	*((volatile unsigned short *) reset_vector) = 0x00;

	if (lapic_read(APIC_ICR1) & APIC_ICR_BUSY) {
		LOG_ERROR("Previous send not complete\n");
		return -EIO;
	}

	// send out INIT to AP
	LOG_DEBUG("Send IPI\n");
	if (has_x2apic()) {
		uint64_t dest = ((uint64_t)id << 32);

		wrmsr(0x800 + (APIC_ICR1 >> 4), dest|APIC_INT_LEVELTRIG|APIC_INT_ASSERT|APIC_DM_INIT);
		if (traditional_delay)
			udelay(200);
		else
			udelay(10);
		// reset INIT
		wrmsr(0x800 + (APIC_ICR1 >> 4), APIC_INT_LEVELTRIG|APIC_DM_INIT);
		if (traditional_delay)
			udelay(10000);
		else
			udelay(10);
		// send out the startup
		wrmsr(0x800 + (APIC_ICR1 >> 4), dest|APIC_DM_STARTUP|(start_eip >> 12));
		if (traditional_delay)
			udelay(200);
		else
			udelay(10);
		// do it again
		wrmsr(0x800 + (APIC_ICR1 >> 4), dest|APIC_DM_STARTUP|(start_eip >> 12));
		if (traditional_delay)
			udelay(200);
		else
			udelay(10);

		LOG_DEBUG("IPI done...\n");

		return 0;
	} else {
		set_ipi_dest(id);
		lapic_write(APIC_ICR1, APIC_INT_LEVELTRIG|APIC_INT_ASSERT|APIC_DM_INIT);
		if (traditional_delay)
			udelay(200);
		else
			udelay(10);
		// reset INIT
		lapic_write(APIC_ICR1, APIC_INT_LEVELTRIG|APIC_DM_INIT);
		if (traditional_delay)
			udelay(10000);
		else
			udelay(10);
		// send out the startup
		set_ipi_dest(id);
		lapic_write(APIC_ICR1, APIC_DM_STARTUP|(start_eip >> 12));
		if (traditional_delay)
			udelay(200);
		else
			udelay(10);
		// do it again
		set_ipi_dest(id);
		lapic_write(APIC_ICR1, APIC_DM_STARTUP|(start_eip >> 12));
		if (traditional_delay)
			udelay(200);
		else
			udelay(10);

		LOG_DEBUG("IPI done...\n");

		i = 0;
	        while((lapic_read(APIC_ICR1) & APIC_ICR_BUSY) && (i < 1000))
			i++; // wait for it to finish, give up eventualy tho

		return ((lapic_read(APIC_ICR1) & APIC_ICR_BUSY) ? -EIO : 0); // did it fail (still delivering) or succeed ?
	}
}

int smp_init(void)
{
	uint32_t i, j;
	int err;

	if (ncores <= 1)
		return -EINVAL;

	LOG_DEBUG("CR0 of core %u: 0x%x\n", apic_cpu_id(), read_cr0());

	/*
	 * dirty hack: Reserve memory for the bootup code.
	 * In a single core enviroment is everythink below 8 MB free.
	 *
	 * Copy 16bit startup code to a 16bit address.
	 * Wakeup the other cores via IPI. They start at this address
	 * in real mode, switch to protected and finally they jump to smp_main.
	 */
	page_map(SMP_SETUP_ADDR, SMP_SETUP_ADDR, PAGE_FLOOR(sizeof(boot_code)) >> PAGE_BITS, PG_RW|PG_GLOBAL);
	vma_add(SMP_SETUP_ADDR, SMP_SETUP_ADDR + PAGE_FLOOR(sizeof(boot_code)), VMA_READ|VMA_WRITE|VMA_CACHEABLE);
	memcpy((void*)SMP_SETUP_ADDR, boot_code, sizeof(boot_code));

	for(i=0; i<sizeof(boot_code); i++)
	{
		if (*((uint32_t*) (SMP_SETUP_ADDR + i)) == 0xDEADBEAF) {
			*((uint32_t*) (SMP_SETUP_ADDR + i)) = (uint32_t) read_cr3();
			break;
		}
	}

	LOG_DEBUG("size of the boot_code %d\n", sizeof(boot_code));

	for(i=1; (i<ncores) && (i<MAX_CORES); i++)
	{
		atomic_int32_set(&current_boot_id, i);

		err = wakeup_ap(SMP_SETUP_ADDR, i);
		if (err)
			LOG_WARNING("Unable to wakeup application processor %d: %d\n", i, err);

		for(j=0; (i >= atomic_int32_read(&cpu_online)) && (j < 1000); j++)
			udelay(1000);

		if (i >= atomic_int32_read(&cpu_online)) {
			LOG_ERROR("Unable to wakeup processor %d, cpu_online %d\n", i, atomic_int32_read(&cpu_online));
			return -EIO;
		}
	}

	LOG_DEBUG("%d cores online\n", atomic_int32_read(&cpu_online));

	return 0;
}
#endif


// How many ticks are used to calibrate the APIC timer
#define APIC_TIMER_CALIBRATION_TICKS	(3)

/*
 * detects the timer frequency of the APIC and restarts
 * the APIC timer with the correct period
 */
int apic_calibration(void)
{
	uint8_t flags;
	uint64_t cycles, old, diff;

	if (BUILTIN_EXPECT(!lapic, 0))
		return -ENXIO;

	const uint64_t cpu_freq_hz = (uint64_t) get_cpu_frequency() * 1000000ULL;
	const uint64_t cycles_per_tick = cpu_freq_hz / (uint64_t) TIMER_FREQ;
	const uint64_t wait_cycles = cycles_per_tick * APIC_TIMER_CALIBRATION_TICKS;

	// disable interrupts to increase calibration accuracy
	flags = irq_nested_disable();

	// start timer with max. counter value
	const uint32_t initial_counter = 0xFFFFFFFF;

	lapic_timer_oneshot();
	lapic_timer_set_counter(initial_counter);

	rmb();
	old = get_rdtsc();

	do {
		rmb();
		cycles = get_rdtsc();
		diff = cycles > old ? cycles - old : old - cycles;
	} while(diff < wait_cycles);

	// Calculate timer increments for desired tick frequency
	icr = (initial_counter - lapic_read(APIC_CCR)) / APIC_TIMER_CALIBRATION_TICKS;
	irq_nested_enable(flags);

	lapic_reset();

	LOG_INFO("APIC calibration determined an ICR of 0x%x\n", icr);

	apic_initialized = 1;
	atomic_int32_inc(&cpu_online);

	if (is_single_kernel()) {
		LOG_INFO("Disable PIC\n");
		// Now, HermitCore is able to use the APIC => Therefore, we disable the PIC
		outportb(0xA1, 0xFF);
		outportb(0x21, 0xFF);
	}

	// only the single-kernel maintains the IOAPIC
	if (ioapic && is_single_kernel()) {
		uint8_t max_entry = ioapic_max_redirection_entry();

		// now lets turn everything else on
		for(uint8_t i = 0; i <= max_entry; i++) {
			if (i != 2)
				ioapic_inton(i, apic_processors[boot_processor]->id);
		}

		// now, we don't longer need the IOAPIC timer and turn it off
		LOG_INFO("Disable IOAPIC timer\n");
		ioapic_intoff(2, apic_processors[boot_processor]->id);
	}

#if MAX_CORES > 1
	if (is_single_kernel())
		smp_init();
#endif

	return 0;
}

static int apic_probe(void)
{
	size_t addr;
	uint32_t i, j, count;
	int isa_bus = -1;
	size_t flags = PG_GLOBAL | PG_RW | PG_PCD;

	// protect apic by NX flags
	if (has_nx())
		flags |= PG_XD;

#if 0
	size_t ebda = search_ebda();
	apic_mp = search_mptable(ebda, ebda+0x400);
	if (apic_mp)
		goto found_mp;
#endif

	apic_mp = search_mptable(0xF0000, 0x100000);
	if (apic_mp)
		goto found_mp;
	apic_mp = search_mptable(0x9F000, 0xA0000);
	if (apic_mp)
		goto found_mp;

found_mp:
	if (!apic_mp) {
		LOG_INFO("Didn't find MP config table\n");
		goto no_mp;
	}

	if (isle < 0) {
		//TODO: add detection of NUMA node
		isle = 0;
	}

	LOG_INFO("Found MP config table at 0x%x\n", apic_mp->mp_config);
	LOG_INFO("System uses Multiprocessing Specification 1.%u\n", apic_mp->version);
	LOG_INFO("MP features 1: %u\n", apic_mp->features[0]);

	if (apic_mp->features[0]) {
		LOG_ERROR("Currently, HermitCore supports only multiprocessing via the MP config tables!\n");
		goto no_mp;
	}

	if (apic_mp->features[1] & 0x80)
		LOG_INFO("PIC mode implemented\n");
	else
		LOG_INFO("Virtual-Wire mode implemented\n");

	apic_config = (apic_config_table_t*) ((size_t) apic_mp->mp_config);
	if (((size_t) apic_config & PAGE_MASK) != ((size_t) apic_mp & PAGE_MASK)) {
		page_map((size_t) apic_config & PAGE_MASK,  (size_t) apic_config & PAGE_MASK, 1, flags);
		vma_add( (size_t) apic_config & PAGE_MASK, ((size_t) apic_config & PAGE_MASK) + PAGE_SIZE, VMA_READ|VMA_WRITE);
	}

	if (!apic_config || strncmp((void*) &apic_config->signature, "PCMP", 4) !=0) {
		LOG_ERROR("Invalid MP config table\n");
		goto no_mp;
	}

	addr = (size_t) apic_config;
	addr += sizeof(apic_config_table_t);

	// does the apic table raise the page boundary? => map additional page
	if (apic_config->entry_count * 20 + addr > ((size_t) apic_config & PAGE_MASK) + PAGE_SIZE)
	{
		page_map(((size_t) apic_config & PAGE_MASK) + PAGE_SIZE, ((size_t) apic_config & PAGE_MASK) + PAGE_SIZE, 1, flags);
		vma_add( ((size_t) apic_config & PAGE_MASK) + PAGE_SIZE, ((size_t) apic_config & PAGE_MASK) + 2*PAGE_SIZE, VMA_READ|VMA_WRITE);
	}

	// search the ISA bus => required to redirect the IRQs
	for(i=0; i<apic_config->entry_count; i++) {
		switch(*((uint8_t*) addr)) {
		case 0:
			addr += 20;
			break;
		case 1: {
				apic_bus_entry_t* mp_bus;

				mp_bus = (apic_bus_entry_t*) addr;
				if (mp_bus->name[0] == 'I' && mp_bus->name[1] == 'S' &&
				    mp_bus->name[2] == 'A')
					isa_bus = i;
			}
			addr += 8;
			break;
		default:
			addr += 8;
		}
	}

	addr = (size_t) apic_config;
	addr += sizeof(apic_config_table_t);

	for(i=0, j=0, count=0; i<apic_config->entry_count; i++) {
		if (*((uint8_t*) addr) == 0) { // cpu entry
			apic_processor_entry_t* cpu = (apic_processor_entry_t*) addr;

			if (j < MAX_APIC_CORES) {
				if (is_single_kernel() && (cpu->cpu_flags & 0x02))
					boot_processor = j;
				if (cpu->cpu_flags & 0x01) { // is the processor usable?
					apic_processors[j] = cpu;
					j++;
				}
			}

			if (cpu->cpu_flags & 0x01)
				count++;
			addr += 20;
		} else if (*((uint8_t*) addr) == 2) { // IO_APIC
			apic_io_entry_t* io_entry = (apic_io_entry_t*) addr;
			ioapic = (ioapic_t*) ((size_t) io_entry->addr);
			LOG_INFO("Found IOAPIC at 0x%x\n", ioapic);
			if (is_single_kernel() && ioapic) {
				page_map(IOAPIC_ADDR, (size_t)ioapic & PAGE_MASK, 1, flags);
				vma_add(IOAPIC_ADDR, IOAPIC_ADDR + PAGE_SIZE, VMA_READ|VMA_WRITE);
				ioapic = (ioapic_t*) IOAPIC_ADDR;
				LOG_INFO("Map IOAPIC to 0x%x\n", ioapic);
				LOG_INFO("IOAPIC version: 0x%x\n", ioapic_version());
				LOG_INFO("Max Redirection Entry: %u\n", ioapic_max_redirection_entry());
			}
			addr += 8;
		} else if (*((uint8_t*) addr) == 3) { // IO_INT
			apic_ioirq_entry_t* extint = (apic_ioirq_entry_t*) addr;
			if (extint->src_bus == isa_bus) {
				irq_redirect[extint->src_irq] = extint->dest_intin;
				LOG_INFO("Redirect irq %u -> %u\n", extint->src_irq,  extint->dest_intin);
			}
			addr += 8;
		} else addr += 8;
	}
	LOG_INFO("Found %u cores\n", count);

	if (count > MAX_CORES) {
		LOG_ERROR("Found too many cores! Increase the macro MAX_CORES!\n");
		goto no_mp;
	}
	ncores = count;
	if (is_single_kernel())
		possible_cpus = count;

check_lapic:
	if (apic_config)
		lapic = apic_config->lapic;
	else if (has_apic())
		lapic = 0xFEE00000;

	if (!lapic)
		goto out;
	LOG_INFO("Found APIC at 0x%x\n", lapic);

	if (has_x2apic()) {
		LOG_INFO("Found and enable X2APIC\n");
		x2apic_enable();
	} else {
		if (page_map(LAPIC_ADDR, (size_t)lapic & PAGE_MASK, 1, flags)) {
			LOG_ERROR("Failed to map APIC to 0x%x\n", LAPIC_ADDR);
			goto out;
		} else {
			LOG_INFO("Mapped APIC 0x%x to 0x%x\n", lapic, LAPIC_ADDR);
			vma_add(LAPIC_ADDR, LAPIC_ADDR + PAGE_SIZE, VMA_READ | VMA_WRITE);
			lapic = LAPIC_ADDR;
		}
	}

	LOG_INFO("Maximum LVT Entry: 0x%x\n", apic_lvt_entries());
	LOG_INFO("APIC Version: 0x%x\n", apic_version());
	LOG_INFO("EOI-broadcast: %s\n", (apic_broadcast()) ? "available" : "unavailable");

	if (!((apic_version() >> 4))) {
		LOG_ERROR("Currently, HermitCore doesn't support external APICs!\n");
		goto out;
	}

	if (apic_lvt_entries() < 3) {
		LOG_ERROR("LVT is too small\n");
		goto out;
	}

	return 0;

out:
	apic_mp = NULL;
	apic_config = NULL;
	lapic = 0;
	ncores = 1;
	return -ENXIO;

no_mp:
	if (isle < 0)
		isle = 0;
	if (boot_processor < 0)
		boot_processor = 0;
	apic_mp = NULL;
	apic_config = NULL;
	if (!is_uhyve())
		ncores = 1;
	goto check_lapic;
}

extern int smp_main(void);
extern void gdt_flush(void);
extern int set_idle_task(void);

#if MAX_CORES > 1
int smp_start(void)
{
	x2apic_enable();

	// reset APIC and set id
	lapic_reset();

	LOG_DEBUG("Processor %d (local id %d) is entering its idle task\n", apic_cpu_id(), atomic_int32_read(&current_boot_id));

	// use the same gdt like the boot processors
	gdt_flush();

	// install IDT
	idt_install();

	// enable additional cpu features
	cpu_detection();

	LOG_DEBUG("CR0 of core %u: 0x%x\n", atomic_int32_read(&current_boot_id), read_cr0());
	online[atomic_int32_read(&current_boot_id)] = 1;

	// set task switched flag for the first FPU access
	// => initialize the FPU
	size_t cr0 = read_cr0();
	cr0 |= CR0_TS;
	write_cr0(cr0);

	set_idle_task();

	/*
	 * TSS is set, pagining is enabled
	 * => now, we are able to register our task
	 */
	register_task();

	irq_enable();

	atomic_int32_inc(&cpu_online);

	return smp_main();
}

int ipi_tlb_flush(void)
{
	uint32_t id = CORE_ID;

	if (atomic_int32_read(&cpu_online) <= 1)
		return 0;

	if (has_x2apic()) {
		uint8_t flags = irq_nested_disable();
		for(uint64_t i=0; i<MAX_APIC_CORES; i++)
		{
			if (i == id)
				continue;
			if (!online[i])
				continue;

			LOG_DEBUG("Send IPI to %zd\n", i);
			wrmsr(0x830, (i << 32)|APIC_INT_ASSERT|APIC_DM_FIXED|112);
		}
		irq_nested_enable(flags);
	} else {
		if (lapic_read(APIC_ICR1) & APIC_ICR_BUSY) {
			LOG_ERROR("Previous send not complete");
			return -EIO;
		}

		uint8_t flags = irq_nested_disable();
		for(uint64_t i=0; i<MAX_APIC_CORES; i++)
		{
			if (i == id)
				continue;
			if (!online[i])
				continue;

			LOG_DEBUG("Send IPI to %zd\n", i);
			set_ipi_dest(i);
			lapic_write(APIC_ICR1, APIC_INT_ASSERT|APIC_DM_FIXED|112);

			uint32_t j = 0;
			while((lapic_read(APIC_ICR1) & APIC_ICR_BUSY) && (j < 1000))
				j++; // wait for it to finish, give up eventualy tho
		}
		irq_nested_enable(flags);
	}

	return 0;
}

static void apic_tlb_handler(struct state *s)
{
	LOG_DEBUG("Receive IPI at core %d to flush the TLB\n", CORE_ID);
	write_cr3(read_cr3());
}
#endif

int apic_send_ipi(uint64_t dest, uint8_t irq)
{
	uint32_t j;
	uint8_t flags;

	if (has_x2apic()) {
		flags = irq_nested_disable();
		LOG_DEBUG("send IPI %d to %lld\n", (int)irq, dest);
		wrmsr(0x830, (dest << 32)|APIC_INT_ASSERT|APIC_DM_FIXED|irq);
		irq_nested_enable(flags);
	} else {
		flags = irq_nested_disable();

		while (lapic_read(APIC_ICR1) & APIC_ICR_BUSY) {
			PAUSE;
		}

		LOG_DEBUG("send IPI %d to %lld\n", (int)irq, dest);
		set_ipi_dest((uint32_t)dest);
		lapic_write(APIC_ICR1, APIC_INT_ASSERT|APIC_DM_FIXED|irq);

		j = 0;
		while((lapic_read(APIC_ICR1) & APIC_ICR_BUSY) && (j < 1000)) {
			j++; // wait for it to finish, give up eventualy tho
			PAUSE;
		}

		irq_nested_enable(flags);
	}

	return 0;
}

static void apic_err_handler(struct state *s)
{
	LOG_ERROR("Got APIC error 0x%x\n", lapic_read(APIC_ESR));
}

void shutdown_system(void)
{
	int if_bootprocessor = (boot_processor == apic_cpu_id());

	irq_disable();

	if (if_bootprocessor) {
		LOG_INFO("Try to shutdown HermitCore\n");

		//vma_dump();
		dump_pstate();

		while(atomic_int32_read(&cpu_online) != 1)
			PAUSE;

		network_shutdown();

		LOG_INFO("Disable APIC timer\n");
	}

	apic_disable_timer();

	if (if_bootprocessor)
		LOG_INFO("Disable APIC\n");

	lapic_write(APIC_LVT_TSR, 0x10000);	// disable thermal sensor interrupt
	lapic_write(APIC_LVT_PMC, 0x10000);	// disable performance counter interrupt
	lapic_write(APIC_SVR, 0x00);	// disable the apic

	// disable x2APIC
	if (if_bootprocessor)
		x2apic_disable();

	if (if_bootprocessor) {
		print_irq_stats();
		LOG_INFO("System goes down...\n");
	}

	flush_cache();
	atomic_int32_dec(&cpu_online);

	while(1) {
		HALT;
	}
}

static void apic_shutdown(struct state* s)
{
	go_down = 1;

	LOG_DEBUG("Receive shutdown interrupt\n");
}

static void apic_wakeup(struct state* s)
{
	LOG_DEBUG("Receive wakeup interrupt\n");
}

int apic_init(void)
{
	int ret;

	ret = apic_probe();
	if (ret)
		return ret;

	// set APIC error handler
	irq_install_handler(121, apic_wakeup);
	irq_install_handler(126, apic_err_handler);
#if MAX_CORES > 1
	irq_install_handler(80+32, apic_tlb_handler);
#endif
	irq_install_handler(81+32, apic_shutdown);
	if (apic_processors[boot_processor])
		LOG_INFO("Boot processor %u (ID %u)\n", boot_processor, apic_processors[boot_processor]->id);
	else
		LOG_INFO("Boot processor %u\n", boot_processor);
	online[boot_processor] = 1;

	return 0;
}

int ioapic_inton(uint8_t irq, uint8_t apicid)
{
	ioapic_route_t route;
	uint32_t off;

	if (BUILTIN_EXPECT(irq > 24, 0)){
		LOG_ERROR("IOAPIC: trying to turn on irq %i which is too high\n", irq);
		return -EINVAL;
	}

	if (irq < 16)
		off = irq_redirect[irq]*2;
	else
		off = irq*2;
#if 0
	route.lower.whole = ioapic_read(IOAPIC_REG_TABLE+1+off);
	route.dest.upper = ioapic_read(IOAPIC_REG_TABLE+off);
	route.lower.bitfield.mask = 0; // turn it on (stop masking)
#else
	route.lower.bitfield.dest_mode = 0;
	route.lower.bitfield.mask = 0;
	route.dest.physical.physical_dest = apicid; // send to the boot processor
	route.lower.bitfield.delivery_mode = 0;
	route.lower.bitfield.polarity = 0;
	route.lower.bitfield.trigger = 0;
	route.lower.bitfield.vector = 0x20+irq;
	route.lower.bitfield.mask = 0; // turn it on (stop masking)
#endif

	ioapic_write(IOAPIC_REG_TABLE+off, route.lower.whole);
	ioapic_write(IOAPIC_REG_TABLE+1+off, route.dest.upper);

	route.dest.upper = ioapic_read(IOAPIC_REG_TABLE+1+off);
        route.lower.whole = ioapic_read(IOAPIC_REG_TABLE+off);

	return 0;
}

int ioapic_intoff(uint8_t irq, uint8_t apicid)
{
	ioapic_route_t route;
	uint32_t off;

	if (BUILTIN_EXPECT(irq > 24, 0)){
		LOG_ERROR("IOAPIC: trying to turn off irq %i which is too high\n", irq);
		return -EINVAL;
	}

	if (irq < 16)
		off = irq_redirect[irq]*2;
	else
		off = irq*2;

#if 0
	route.lower.whole = ioapic_read(IOAPIC_REG_TABLE+1+off);
	route.dest.upper = ioapic_read(IOAPIC_REG_TABLE+off);
	route.lower.bitfield.mask = 1; // turn it off (start masking)
#else
	route.lower.bitfield.dest_mode = 0;
	route.lower.bitfield.mask = 0;
	route.dest.physical.physical_dest = apicid;
	route.lower.bitfield.delivery_mode = 0;
	route.lower.bitfield.polarity = 0;
	route.lower.bitfield.trigger = 0;
	route.lower.bitfield.vector = 0x20+irq;
	route.lower.bitfield.mask = 1; // turn it off (start masking)
#endif

	ioapic_write(IOAPIC_REG_TABLE+off, route.lower.whole);
	ioapic_write(IOAPIC_REG_TABLE+1+off, route.dest.upper);

	return 0;
}
