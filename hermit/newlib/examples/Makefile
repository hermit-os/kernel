ARCH = x86
TARGET=x86_64-hermit
MAKE = make
STRIP_DEBUG = --strip-debug
KEEP_DEBUG = --only-keep-debug

# Set your own cross compiler tool chain prefix here
override CROSSCOMPREFIX = ../$(ARCH)/bin/$(TARGET)-

override CC_FOR_TARGET = $(CROSSCOMPREFIX)gcc
override CXX_FOR_TARGET = $(CROSSCOMPREFIX)g++
override GCC_FOR_TARGET = $(CROSSCOMPREFIX)gcc
override CPP_FOR_TARGET = $(CROSSCOMPREFIX)cpp
override AR_FOR_TARGET = $(CROSSCOMPREFIX)ar
override AS_FOR_TARGET = $(CROSSCOMPREFIX)as
override LD_FOR_TARGET = $(CROSSCOMPREFIX)ld
override NM_FOR_TARGET = $(CROSSCOMPREFIX)nm
override OBJDUMP_FOR_TARGET = $(CROSSCOMPREFIX)objdump
override OBJCOPY_FOR_TARGET = $(CROSSCOMPREFIX)objcopy
override RANLIB_FOR_TARGET = $(CROSSCOMPREFIX)ranlib
override STRIP_FOR_TARGET = $(CROSSCOMPREFIX)strip
override READELF_FOR_TARGET = $(CROSSCOMPREFIX)readelf

override CFLAGS=-O3 -mtune=native

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
