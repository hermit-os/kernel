TERM = xterm
TOPDIR := $(shell pwd)
ARCH = x86
NAME = hermit
KERNDIRS = kernel mm libkern fs arch/$(ARCH)/kernel arch/$(ARCH)/mm
SUBDIRS = $(KERNDIRS)
GIT_VERSION := $(shell git describe --abbrev=6 --dirty --always --tags)
TODAY := $(shell date +'%Y%m%d')

# Set your own cross compiler tool chain prefix here
CROSSCOMPREFIX =
STACKPROT = -fno-stack-protector

CC_FOR_TARGET = $(CROSSCOMPREFIX)gcc
CXX_FOR_TARGET = $(CROSSCOMPREFIX)g++
GCC_FOR_TARGET = $(CROSSCOMPREFIX)gcc
CPP_FOR_TARGET = $(CROSSCOMPREFIX)cpp
AR_FOR_TARGET = $(CROSSCOMPREFIX)ar
AS_FOR_TARGET = $(CROSSCOMPREFIX)as
LD_FOR_TARGET = $(CROSSCOMPREFIX)ld
NM_FOR_TARGET = $(CROSSCOMPREFIX)nm
OBJDUMP_FOR_TARGET = $(CROSSCOMPREFIX)objdump
OBJCOPY_FOR_TARGET = $(CROSSCOMPREFIX)objcopy
RANLIB_FOR_TARGET = $(CROSSCOMPREFIX)ranlib
STRIP_FOR_TARGET = $(CROSSCOMPREFIX)strip
READELF_FOR_TARGET = $(CROSSCOMPREFIX)readelf

MAKE = make
NASM = nasm
NASMFLAGS = -felf64 -g -i$(TOPDIR)/include/hermit/
GDB = $(CROSSCOMPREFIX)gdb
GDBFLAGS = -x debug.gdb
#QEMU = qemu-system-x86_64
QEMU = qemu-kvm -cpu host
QEMUFLAGS = -smp 2 -monitor stdio \
	    -net nic,model=rtl8139 \
	    -net user,hostfwd=tcp::12345-:7
QEMUDEBUGFLAGS = -monitor none -daemonize \
	    -net nic,model=rtl8139 \
	    -net user,hostfwd=tcp::12345-:7
QEMUSERIALFLAGS = -device pci-serial,chardev=tS0 \
		-chardev socket,host=localhost,port=4555,server,id=tS0

INCLUDE = -I$(TOPDIR)/include -I$(TOPDIR)/arch/$(ARCH)/include  -I$(TOPDIR)/lwip/src/include -I$(TOPDIR)/lwip/src/include/ipv4
# Compiler options for final code
CFLAGS = -DVERSION=\"$(GIT_VERSION)\" -D_HERMIT -g -m64 -Wall -O2 -mno-red-zone -fno-var-tracking-assignments -fstrength-reduce -fomit-frame-pointer -finline-functions -ffreestanding -nostdinc -fno-stack-protector $(INCLUDE)
# Compiler options for debugging
debug debug-eclipse : CFLAGS = -DVERSION=\"$(GIT_VERSION)\" -D_HERMIT -g -O0 -m64 -Wall -fno-builtin -DWITH_FRAME_POINTER -nostdinc -mno-red-zone -fno-stack-protector $(INCLUDE)
AR = ar
ARFLAGS = rsv
RM = rm -rf
LDFLAGS = -T link.ld -z max-page-size=4096 --defsym __BUILD_DATE=$(TODAY) -nostdlib
STRIP_DEBUG = --strip-debug
KEEP_DEBUG = --only-keep-debug
OUTPUT_FORMAT = -O elf32-i386

CFLAGS_FOR_NEWLIB = $(STACKPROT) #-mtune=native
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

all: newlib tools $(NAME).elf

newlib:
	$Q$(MAKE) ARCH=$(ARCH) \
		LDFLAGS="$(LDFLAGS_FOR_NEWLIB)" \
		CFLAGS="$(CFLAGS_FOR_NEWLIB)" \
		NASMFLAGS="$(NASMFLAGS_FOR_NEWLIB)" \
		CC_FOR_TARGET=$(CC_FOR_TARGET) \
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
		READELF_FOR_TARGET=$(READELF_FOR_TARGET) -C newlib

tools:
	$Q$(MAKE) ARCH=$(ARCH) CFLAGS="$(CFLAGS_FOR_TOOLS)" LDFLAGS="$(LDFLAGS_FOR_TOOLS)" -C tools

$(NAME).elf:
	@echo [LD] $(NAME).elf
	$Q$(LD_FOR_TARGET) $(LDFLAGS) -o $(NAME).elf $^ tools/initrd.o
	@echo [OBJCOPY] $(NAME).bin
	$Q$(OBJCOPY_FOR_TARGET) -j .mboot -j .kmsg -j .initrd -j .text -j .text.startup -j .data -j .rodata -j .bss -O binary $(NAME).elf $(NAME).bin

$(NAME).elf32: $(NAME).elf
	@echo [OBJCOPY] $(NAME).sym
	$Q$(OBJCOPY_FOR_TARGET) $(KEEP_DEBUG) $(NAME).elf $(NAME).sym
	@echo [OBJCOPY] $(NAME).elf32
	$Q$(OBJCOPY_FOR_TARGET) $(STRIP_DEBUG) $(OUTPUT_FORMAT) $(NAME).elf $(NAME).elf32

clean:
	$Q$(RM) $(NAME).elf $(NAME).sym $(NAME).bin *~
	$Q$(MAKE) -C tools clean
	$Q$(MAKE) -C newlib clean
	@echo Cleaned.

veryclean: clean
	$Q$(RM) qemu-vlan0.pcap include/hermit/config.inc
	$Q$(MAKE) -C tools veryclean
	$Q$(MAKE) -C newlib veryclean
	@echo Very cleaned

qemu: $(NAME).elf32
	$(QEMU) $(QEMUFLAGS) -kernel $(NAME).elf32

debug: $(NAME).elf32
	$(TERM) -e $(GDB) $(GDBFLAGS) &
	$(QEMU) $(QEMUDEBUGFLAGS) -s -S -kernel $(NAME).elf32

doc:
	@echo Create documentation...
	@doxygen

%.o : %.c
	@echo [CC] $@
	$Q$(CC_FOR_TARGET) -c -D__KERNEL__ $(CFLAGS) -o $@ $<
	@echo [DEP] $*.dep
	$Q$(CC_FOR_TARGET) -MF $*.dep -MT $*.o -MM $(CFLAGS) $<

include/hermit/config.inc: include/hermit/config.h
	@echo "; This file is generated automatically from the config.h file." > include/hermit/config.inc
	@echo "; Before editing this, you should consider editing config.h." >> include/hermit/config.inc
	@awk '/^#define MAX_CORES/{ print "%define MAX_CORES", $$3 }' include/hermit/config.h >> include/hermit/config.inc
	@awk '/^#define KERNEL_STACK_SIZE/{ print "%define KERNEL_STACK_SIZE", $$3 }' include/hermit/config.h >> include/hermit/config.inc
	@awk '/^#define VIDEO_MEM_ADDR/{ print "%define VIDEO_MEM_ADDR", $$3 }' include/hermit/config.h >> include/hermit/config.inc

%.o : %.asm include/hermit/config.inc
	@echo [ASM] $@
	$Q$(NASM) $(NASMFLAGS) -o $@ $<

%.o : %.S
	@echo [GCC-ASM] $@
	$Q$(CC_FOR_TARGET) $(CFLAGS) -c -o $@ $<

.PHONY: default all clean emu gdb newlib tools

include $(addsuffix /Makefile,$(SUBDIRS))
