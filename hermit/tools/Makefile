MAKE = make
NASM = nasm
NASMFLAGS = -fbin
CC = gcc
CFLAGS = -O2 -Wall
HEXDUMP = hexdump
LDFLGAS =
INITRDFILES = ../usr/tests/hello #$(shell find ../usr/tests -perm -u+r+x -type f)
PROXYFILES = $(shell find ../usr/tests -perm -u+r+x -type f)

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
	
all: init.h make_initrd initrd.o proxy

init.bin: init.asm
	@echo [NASM] $@
	$Q$(NASM) $(NASMFLAGS) -o $@ $<

init.h: init.bin
	@echo [HEXDUMP] $@
	$Q@echo "static const uint8_t boot_code[] = {" > $@
	$Q$(HEXDUMP) -v -e '"0x" 1/1 "%02X" ", "' $< >> $@
	$Q@echo "};" >> $@

initrd.o: initrd.S initrd.img
	@echo [CC] Build initrd as object file
	$Q$(CC) -c -o $@ initrd.S
 
initrd.img: $(INITRDFILES) make_initrd
	@echo [MAKE_INITRD] initrd.img
	$Q./make_initrd /bin $(foreach FILE, $(INITRDFILES), $(FILE) $(shell basename $(FILE)))

make_initrd: make_initrd.o
	$Q$(CC) $(CFLAGS) -o $@ $< $(LDFLAGS)

proxy: $(PROXYFILES)
	@echo [PROXY] Create proxy files
	$Q./create_proxy.sh $(foreach FILE, $(PROXYFILES), $(FILE))

clean:
	@echo Cleaning tools
	$Q$(RM) -rf *.o *~ *.bin *.obj *.h

veryclean: clean
	

depend:
	$(CC) -MM $(CFLAGS) *.c > Makefile.dep

-include Makefile.dep
# DO NOT DELETE
