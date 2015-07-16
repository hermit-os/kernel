ARCH = x86
NEWLIB = ../x86/x86_64-hermit-elf64
MAKE = make
STRIP_DEBUG = --strip-debug
KEEP_DEBUG = --only-keep-debug

override LDFLAGS += -T link.ld

# Default section offsets in x86-64 ELF files are aligned to the page-size.
# For x86-64 the pagesize is huge (2 MB) with the consquence of large sparse
# ELF files (which lead to a huge initrd). To solve this, we manually set the page-size to 4 KB.
# Furthermore, we use a large memory model to deal with real 64bit application.
override LDFLAGS += -Wl,-n,-z,max-page-size=0x1000

# Prettify output
V = 0
ifeq ($V,0)
	Q = @
	P = > /dev/null
endif

# other implicit rules
%.o : %.c
	@echo [CC] $@
	$Q$(CC_FOR_TARGET) -c $(CFLAGS) -o $@ $< 

default: all

all: hello jacobi stream

stream: stream.o
	@echo [LD] $@
	$Q$(CC_FOR_TARGET) $(LDFLAGS) $(CFLAGS) -o $@ $<
	$Q$(OBJCOPY_FOR_TARGET) $(KEEP_DEBUG) $@ $@.sym
	$Q$(OBJCOPY_FOR_TARGET) $(STRIP_DEBUG) $@
	$Qchmod a-x $@.sym

hello: hello.o
	@echo [LD] $@
	$Q$(CC_FOR_TARGET) $(LDFLAGS) $(CFLAGS) -o $@ $<
	$Q$(OBJCOPY_FOR_TARGET) $(KEEP_DEBUG) $@ $@.sym
	$Q$(OBJCOPY_FOR_TARGET) $(STRIP_DEBUG) $@
	$Qchmod a-x $@.sym

jacobi: jacobi.o
	@echo [LD] $@
	$Q$(CC_FOR_TARGET) $(LDFLAGS) $(CFLAGS) -o $@ $< -lm
	$Q$(OBJCOPY_FOR_TARGET) $(KEEP_DEBUG) $@ $@.sym
	$Q$(OBJCOPY_FOR_TARGET) $(STRIP_DEBUG) $@
	$Qchmod a-x $@.sym

clean:
	@echo Cleaning examples
	$Q$(RM) hello jacobi *.sym *.o *~ 

veryclean:
	@echo Propper cleaning examples
	$Q$(RM) hello jacobi stream *.sym *.o *~

depend:
	$Q$(CC_FOR_TARGET) -MM $(CFLAGS) *.c > Makefile.dep

-include Makefile.dep
# DO NOT DELETE
