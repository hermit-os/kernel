MAKE = make
NASM = nasm
NASMFLAGS = -fbin
CC = gcc
CFLAGS = -O2 -Wall
HEXDUMP = hexdump
LDFLAGS = -lelf
PROXYFILES = proxy $(shell find ../usr/tests ../usr/benchmarks ../usr/openmpbench -type f -executable)

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

all: proxy-image.iso init.h

init.bin: init.asm
	@echo [NASM] $@
	$Q$(NASM) $(NASMFLAGS) -o $@ $<

init.h: init.bin
	@echo [HEXDUMP] $@
	$Q@echo "static const uint8_t boot_code[] = {" > $@
	$Q$(HEXDUMP) -v -e '"0x" 1/1 "%02X" ", "' $< >> $@
	$Q@echo "};" >> $@

iso:
	mkdir -p iso

proxy-image.iso: iso $(PROXYFILES)
	$Qcp $(PROXYFILES) iso
	$Qgenisoimage -R -J -input-charset utf8 -o proxy-image.iso iso

clean:
	@echo Cleaning tools
	$Q$(RM) -rf *.o *~ *.bin *.obj *.h *.iso iso

veryclean: clean
	$Q$(RM) -rf iso

depend:
	$(CC) -MM $(CFLAGS) *.c > Makefile.dep

-include Makefile.dep
# DO NOT DELETE
