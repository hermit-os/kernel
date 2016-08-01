TERM = xterm
TOPDIR := $(shell pwd)
ARCH = x86
NAME = libhermit.a
DRIVERDIRS = drivers/net
LWIPDIRS = lwip/src/arch lwip/src/api lwip/src/core lwip/src/core/ipv4 lwip/src/netif
KERNDIRS = kernel mm libkern arch/$(ARCH)/kernel arch/$(ARCH)/mm $(LWIPDIRS) $(DRIVERDIRS)
SUBDIRS = $(KERNDIRS)
GIT_VERSION := $(shell git describe --abbrev=6 --dirty --always --tags)
TODAY := $(shell date +'%Y%m%d')
INITRAMFS=../../busybox-1.23.2/initramfs/

# Set your own cross compiler tool chain prefix here
CROSSCOMPREFIX = x86_64-hermit
STACKPROT = -fno-stack-protector

FC_FOR_TARGET = $(CROSSCOMPREFIX)-gfortran
CC_FOR_TARGET = $(CROSSCOMPREFIX)-gcc
CXX_FOR_TARGET = $(CROSSCOMPREFIX)-g++
GCC_FOR_TARGET = $(CROSSCOMPREFIX)-gcc
CPP_FOR_TARGET = $(CROSSCOMPREFIX)-cpp
AR_FOR_TARGET = $(CROSSCOMPREFIX)-ar
AS_FOR_TARGET = $(CROSSCOMPREFIX)-as
LD_FOR_TARGET = $(CROSSCOMPREFIX)-ld
NM_FOR_TARGET = $(CROSSCOMPREFIX)-nm
OBJDUMP_FOR_TARGET = $(CROSSCOMPREFIX)-objdump
OBJCOPY_FOR_TARGET = $(CROSSCOMPREFIX)-objcopy
RANLIB_FOR_TARGET = $(CROSSCOMPREFIX)-ranlib
STRIP_FOR_TARGET = $(CROSSCOMPREFIX)-strip
READELF_FOR_TARGET = $(CROSSCOMPREFIX)-readelf
ELFEDIT_FOR_TARGET = $(CROSSCOMPREFIX)-elfedit

MAKE = make
NASM = nasm
NASMFLAGS = -felf64 -g -i$(TOPDIR)/include/hermit/

INCLUDE = -I$(TOPDIR)/include -I$(TOPDIR)/arch/$(ARCH)/include  -I$(TOPDIR)/lwip/src/include -I$(TOPDIR)/lwip/src/include/ipv4 -I$(TOPDIR)/drivers
CFLAGS = -DVERSION=\"$(GIT_VERSION)\" -g -m64 -Wall -O2 -mno-red-zone -fno-var-tracking-assignments -fstrength-reduce -fomit-frame-pointer -finline-functions -ffreestanding -nostdinc -fno-stack-protector -mno-sse -mno-mmx -mno-sse2 -mno-3dnow -mno-avx -fno-delete-null-pointer-checks -falign-jumps=1 -falign-loops=1 -mno-80387 -mno-fp-ret-in-387 -mskip-rax-setup -fno-common -Wframe-larger-than=1024 -fno-strict-aliasing -fno-asynchronous-unwind-tables -fno-strict-overflow -maccumulate-outgoing-args $(INCLUDE)
AR = ar
ARFLAGS = rsv
RM = rm -rf
OUTPUT_FORMAT = -O elf64-x86-64-hermit

# Additional flags for profiling using Xray
PROFILING_LDFLAGS =
PROFILING_CFLAGS =

ifdef PROFILING
    PROFILING_LDFLAGS = -lxray

    PROFILING_CFLAGS  = -falign-functions=32 -finstrument-functions
    PROFILING_CFLAGS += -finstrument-functions-exclude-function-list=_mm_pause,_mm_setcsr,_mm_getcsr # we need this for libiomp to work
    PROFILING_CFLAGS += -DXRAY -DXRAY_DISABLE_BROWSER_INTEGRATION -DXRAY_NO_DEMANGLE
    PROFILING_CFLAGS += -DXRAY_ANNOTATE
endif

CFLAGS_FOR_NEWLIB = -m64 -mtls-direct-seg-refs -O3 -march=native -mtune=native -ftree-vectorize #$(STACKPROT)
FCFLAGS_FOR_NEWLIB = -m64 -mtls-direct-seg-refs -O3 -march=native -mtune=native -ftree-vectorize
FFLAGS_FOR_NEWLIB = -m64 -mtls-direct-seg-refs -O3 -march=native -mtune=native -ftree-vectorize
CXXFLAGS_FOR_NEWLIB = -m64 -mtls-direct-seg-refs -O3 -march=native -mtune=native -ftree-vectorize
LDFLAGS_FOR_NEWLIB =
NASMFLAGS_FOR_NEWLIB = -felf64
CFLAGS_FOR_TOOLS = -O2 -Wall
LDFLAGS_FOR_TOOLS =

# Prettify output
V = 0
ifeq ($V,0)
	Q = @
	P = > /dev/null
endif


default: all

all: arch/x86/kernel/boot.h bootstrap $(NAME) toolchain tools

toolchain:
	$Q$(MAKE) ARCH=$(ARCH) \
		LDFLAGS_FOR_TARGET="$(LDFLAGS_FOR_NEWLIB)" \
		CFLAGS_FOR_TARGET="$(CFLAGS_FOR_NEWLIB)" \
		FFLAGS_FOR_TARGET="$(FFLAGS_FOR_NEWLIB)" \
		FCFLAGS_FOR_TARGET="$(FCFLAGS_FOR_NEWLIB)" \
		FCFLAGS="$(FCFLAGS_FOR_NEWLIB)" \
		CXXFLAGS_FOR_TARGET="$(CXXFLAGS_FOR_NEWLIB)" \
		NASMFLAGS="$(NASMFLAGS_FOR_NEWLIB)" \
		CC_FOR_TARGET=$(CC_FOR_TARGET) \
		FC_FOR_TARGET=$(FC_FOR_TARGET) \
		CXX_FOR_TARGET=$(CXX_FOR_TARGET) \
		GCC_FOR_TARGET=$(GCC_FOR_TARGET) \
		AR_FOR_TARGET=$(AR_FOR_TARGET) \
		AS_FOR_TARGET=$(AS_FOR_TARGET) \
		LD_FOR_TARGET=$(LD_FOR_TARGET) \
		NM_FOR_TARGET=$(NM_FOR_TARGET) \
		OBJDUMP_FOR_TARGET=$(OBJDUMP_FOR_TARGET) \
		OBJCOPY_FOR_TARGET=$(OBJCOPY_FOR_TARGET) \
		RANLIB_FOR_TARGET=$(RANLIB_FOR_TARGET) \
		STRIP_FOR_TARGET=$(STRIP_FOR_TARGET) \
		ELFEDIT_FOR_TARGET=$(ELFEDIT_FOR_TARGET) \
		READELF_FOR_TARGET=$(READELF_FOR_TARGET) \
		PROFILING_CFLAGS="$(PROFILING_CFLAGS)" \
		PROFILING_LDFLAGS="$(PROFILING_LDFLAGS)" -C usr toolchain

bootstrap:
	$Q$(MAKE) ARCH=$(ARCH) CFLAGS="" LDFLAGS="" -C usr bootstrap

tools:
	$Q$(MAKE) ARCH=$(ARCH) CFLAGS="$(CFLAGS_FOR_TOOLS)" LDFLAGS="$(LDFLAGS_FOR_TOOLS)" -C tools

$(NAME):
	@echo [AR] $(NAME)
	$Q$(AR_FOR_TARGET) $(ARFLAGS) -o $(NAME) $^
	$Q$(OBJCOPY_FOR_TARGET) --rename-section .bss=.kbss --rename-section .text=.ktext --rename-section .data=.kdata $(NAME) $(TOPDIR)/usr/$(ARCH)/$(CROSSCOMPREFIX)/lib/$(NAME)

clean:
	$Q$(RM) $(NAME) $(NAME).sym $(NAME).bin *~
	$Q$(MAKE) -C tools clean
	$Q$(MAKE) -C usr clean
	@echo Cleaned.

veryclean: clean
	$Q$(RM) qemu-vlan0.pcap include/hermit/config.inc
	$Q$(MAKE) -C tools veryclean
	$Q$(MAKE) -C usr veryclean
	@echo Very cleaned

doc:
	@echo Create documentation...
	@doxygen

%.o : %.c
	@echo [CC] $@
	$Q$(CC_FOR_TARGET) -c -D__KERNEL__ $(CFLAGS) -o $@ $<
	@echo [DEP] $*.dep
	$Q$(CC_FOR_TARGET) -MF $*.dep -MT $*.o -MM $(CFLAGS) $<

arch/x86/kernel/boot.bin: arch/x86/kernel/boot.asm
	@echo "  NASM    $@"
	$Q$(NASM) -fbin -o $@ $<

arch/x86/kernel/boot.h: arch/x86/kernel/boot.bin
	@echo "  HEXDUMP $@"
	$Qecho "static const uint8_t boot_code[] = { \
		$(shell hexdump -v -e '"0x" 1/1 "%02X" ", "' $<) \
	};" > $@

include/hermit/config.inc: include/hermit/config.h
	@echo "; This file is generated automatically from the config.h file." > include/hermit/config.inc
	@echo "; Before editing this, you should consider editing config.h." >> include/hermit/config.inc
	@awk '/^#define MAX_CORES/{ print "%define MAX_CORES", $$3 }' include/hermit/config.h >> include/hermit/config.inc
	@awk '/^#define KERNEL_STACK_SIZE/{ print "%define KERNEL_STACK_SIZE", $$3 }' include/hermit/config.h >> include/hermit/config.inc
	@awk '/^#define VIDEO_MEM_ADDR/{ print "%define VIDEO_MEM_ADDR", $$3 }' include/hermit/config.h >> include/hermit/config.inc
	@awk '/^#define CONFIG_VGA/{ print "%define CONFIG_VGA", $$3 }' include/hermit/config.h >> include/hermit/config.inc
	@awk '/^#define DYNAMIC_TICKS/{ print "%define DYNAMIC_TICKS", $$3 }' include/hermit/config.h >> include/hermit/config.inc
	@awk '/^#define SAVE_FPU/{ print "%define SAVE_FPU", $$3 }' include/hermit/config.h >> include/hermit/config.inc

%.o : %.asm include/hermit/config.inc
	@echo [ASM] $@
	$Q$(NASM) $(NASMFLAGS) -o $@ $<
	$Q$(ELFEDIT_FOR_TARGET) --output-osabi HermitCore $@
	#$Q$(OBJCOPY_FOR_TARGET) $(OUTPUT_FORMAT) $@

%.o : %.S
	@echo [GCC-ASM] $@
	$Q$(CC_FOR_TARGET) $(CFLAGS) -c -o $@ $<

.PHONY: default all clean emu gdb usr toolchain tools

include $(addsuffix /Makefile,$(SUBDIRS))
