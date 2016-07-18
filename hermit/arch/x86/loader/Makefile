MAKE = make
QEMU = qemu-system-x86_64 -machine accel=kvm -cpu host
OBJDUMP = objdump
OBJCOPY = objcopy
CC = gcc
CFLAGS = -O2 -Wall -m64 -ffreestanding -mno-red-zone -fstrength-reduce -fomit-frame-pointer -finline-functions -Iinclude
NASM = nasm
NASMFLAGS = -felf64 -g
LD = ld
LDFLAGS = -T link.ld -z max-page-size=4096
STRIP_DEBUG = --strip-debug
KEEP_DEBUG = --only-keep-debug
OUTPUT_FORMAT = -O elf32-i386
NAME = ldhermit

C_source := main.c printf.c string.c stdio.c vga.c page.c
ASM_source := entry.asm

OBJS := $(C_source:.c=.o)
OBJS += $(ASM_source:.asm=.o)

# Prettify output
V = 0
ifeq ($V,0)
	Q = @
	P = > /dev/null
endif

# other implicit rules
%.o : %.c
	@echo [CC] $@
	$Q$(CC) -c $(CFLAGS) -o $@ $<
	@echo [DEP] $*.dep
	$Q$(CC) -MF $*.dep -MT $*.o -MM $(CFLAGS) $<

%.o : %.asm
	@echo [ASM] $@
	$Q$(NASM) $(NASMFLAGS) -o $@ $<

all: ldhermit.elf

$(NAME).elf: $(OBJS)
	@echo [LD] $@
	$Q$(LD) $(LDFLAGS) -o $@ $(OBJS)
	@echo [OBJCOPY] $(NAME).sym
	$Q$(OBJCOPY) $(KEEP_DEBUG) $(NAME).elf $(NAME).sym
	@echo [OBJCOPY] $(NAME).elf
	$Q$(OBJCOPY) $(STRIP_DEBUG) $(OUTPUT_FORMAT) $(NAME).elf

qemu:
	$(QEMU) -smp 10 -m 4G -kernel $(NAME).elf -initrd ../../../usr/tests/hello \
		-net nic,model=rtl8139 -net user -net dump \
		-curses -monitor telnet:127.0.0.1:1235,server,nowait \
		-s

clean:
	@echo Cleaning loader
	$Q$(RM) -rf *.o *~ *.bin *.obj

veryclean: clean

depend:
	$(CC) -MM $(CFLAGS) *.c > Makefile.dep

-include -include $(C_source:.c=.dep)
# DO NOT DELETE
