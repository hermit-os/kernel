MAKE = make
NASM = nasm
NASMFLAGS = -fbin
CC = gcc
CFLAGS = -O2 -Wall
HEXDUMP = hexdump
LDFLGAS =
PROXYFILES = $(shell find ../usr/tests -perm -u+r+x -type f) $(shell find ../usr/benchmarks -perm -u+r+x -type f)

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

default: all
	
all: init.h

init.bin: init.asm
	@echo [NASM] $@
	$Q$(NASM) $(NASMFLAGS) -o $@ $<

init.h: init.bin
	@echo [HEXDUMP] $@
	$Q@echo "static const uint8_t boot_code[] = {" > $@
	$Q$(HEXDUMP) -v -e '"0x" 1/1 "%02X" ", "' $< >> $@
	$Q@echo "};" >> $@

iso:
	mkdir iso

proxy: iso $(PROXYFILES)
	@echo [PROXY] Create proxy files
	$Q./create_proxy.sh $(foreach FILE, $(PROXYFILES), $(FILE))
	$Qmv *_proxy iso/
	$Qcp ../hermit.bin iso
	$Qgenisoimage -R -J -input-charset utf8 -o proxy-image.iso iso

clean:
	@echo Cleaning tools
	$Q$(RM) -rf *.o *~ *.bin *.obj *.h

veryclean: clean
	$Q$(RM) -rf iso

depend:
	$(CC) -MM $(CFLAGS) *.c > Makefile.dep

-include Makefile.dep
# DO NOT DELETE
