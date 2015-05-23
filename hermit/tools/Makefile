MAKE = make
NASM = nasm
NASMFLAGS = -fbin

default: all
	
all: init.hex

init.bin: init.asm
	$(NASM) $(NASMFLAGS) -o $@ $<

init.hex: init.bin
	hexdump -v -e '"0x" 1/1 "%02X" ", "' $< > $@

clean:
	$(RM) -rf *.o *~ *.bin *.obj *.hex

depend:
	$(CC) -MM $(CFLAGS) *.c > Makefile.dep

-include Makefile.dep
# DO NOT DELETE
