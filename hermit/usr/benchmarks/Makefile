ARCH = x86
TARGET=x86_64-hermit
MAKE = make
override STRIP_DEBUG = --strip-debug #--strip-unneeded
KEEP_DEBUG = --only-keep-debug

# Set your own cross compiler tool chain prefix here
CROSSCOMPREFIX = x86_64-hermit-

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
ELFEDIT_FOR_TARGET = $(CROSSCOMPREFIX)elfedit

# Prettify output
V = 0
ifeq ($V,0)
	Q = @
	P = > /dev/null
endif

ICC_EXE =
#ICC_EXE = stream_icc

# other implicit rules
%.o : %.c
	@echo [CC] $@
	$Q$(CC_FOR_TARGET) -c $(CFLAGS_FOR_TARGET) -o $@ $< 

%.o: %.cpp
	@echo [CXX] $@
	$Q$(CXX_FOR_TARGET) -c $(CXXFLAGS_FOR_TARGET) $(CFLAGS_FOR_TARGET) -o $@ $<

%.o: %.f90
	@echo [F90] $@
	$Q$(FC_FOR_TARGET) -c $(FFLAGS_FOR_TARGET) -o $@ $<

default: all

all: stream hg netio RCCE_pingping RCCE_pingpong basic $(ICC_EXE)

stream.o: stream.c
	@echo [CC] $@
	$Q$(CC_FOR_TARGET) -c $(CFLAGS_FOR_TARGET) -fopenmp -o $@ $<

stream: stream.o
	@echo [LD] $@
	$Q$(CC_FOR_TARGET) -o $@ $< $(LDFLAGS_FOR_TARGET) $(CFLAGS_FOR_TARGET) -fopenmp
	$Q$(OBJCOPY_FOR_TARGET) $(KEEP_DEBUG) $@ $@.sym
	$Q$(OBJCOPY_FOR_TARGET) $(STRIP_DEBUG) $@
	$Qchmod a-x $@.sym

stream_icc.o: stream.c
	@echo [ICC] $@
	icc -m64 -c -o $@ -mno-red-zone -O3 -xHost -fno-stack-protector -I$(TOPDIR)/hermit/usr/x86/x86_64-hermit/include/ -openmp -ffreestanding $<
	$Q$(ELFEDIT_FOR_TARGET) --output-osabi HermitCore $@

stream_icc: stream_icc.o
	@echo [LD]
	$Q$(CC_FOR_TARGET) -o $@ $< $(LDFLAGS_FOR_TARGET) $(CFLAGS_FOR_TARGET) -fopenmp
	$Q$(OBJCOPY_FOR_TARGET) $(KEEP_DEBUG) $@ $@.sym
	$Q$(OBJCOPY_FOR_TARGET) $(STRIP_DEBUG) $@
	$Qchmod a-x $@.sym

basic.o: basic.c
	@echo [CC] $@
	$Q$(CC_FOR_TARGET) -c $(CFLAGS_FOR_TARGET) -pthread -o $@ $<

basic: basic.o
	@echo [LD] $@
	$Q$(CC_FOR_TARGET) -o $@ $< $(LDFLAGS_FOR_TARGET) $(CFLAGS_FOR_TARGET) -pthread
	$Q$(OBJCOPY_FOR_TARGET) $(KEEP_DEBUG) $@ $@.sym
	$Q$(OBJCOPY_FOR_TARGET) $(STRIP_DEBUG) $@
	$Qchmod a-x $@.sym

RCCE_pingping: RCCE_pingping.o
	@echo [LD] $@
	$Q$(CC_FOR_TARGET) -o $@ $< $(LDFLAGS_FOR_TARGET) $(CFLAGS_FOR_TARGET) -lircce
	$Q$(OBJCOPY_FOR_TARGET) $(KEEP_DEBUG) $@ $@.sym
	$Q$(OBJCOPY_FOR_TARGET) $(STRIP_DEBUG) $@
	$Qchmod a-x $@.sym

RCCE_pingpong: RCCE_pingpong.o
	@echo [LD] $@
	$Q$(CC_FOR_TARGET) -o $@ $< $(LDFLAGS_FOR_TARGET) $(CFLAGS_FOR_TARGET) -lircce
	$Q$(OBJCOPY_FOR_TARGET) $(KEEP_DEBUG) $@ $@.sym
	$Q$(OBJCOPY_FOR_TARGET) $(STRIP_DEBUG) $@
	$Qchmod a-x $@.sym

netio.o: netio.c
	@echo [CC] $@
	$Q$(CC_FOR_TARGET) -c $(CFLAGS_FOR_TARGET) -D_POSIX_SOURCE -o $@ $<

netio: netio.o
	@echo [LD] $@
	$Q$(CC_FOR_TARGET) -o $@ $< $(LDFLAGS_FOR_TARGET) $(CFLAGS_FOR_TARGET)
	$Q$(OBJCOPY_FOR_TARGET) $(KEEP_DEBUG) $@ $@.sym
	$Q$(OBJCOPY_FOR_TARGET) $(STRIP_DEBUG) $@
	$Qchmod a-x $@.sym

hg: hg.o hist.o rdtsc.o run.o init.o opt.o report.o setup.o
	@echo [LD] $@
	$Q$(CC_FOR_TARGET) -o $@ $< $(LDFLAGS_FOR_TARGET) $(CFLAGS_FOR_TARGET) hist.o rdtsc.o run.o init.o opt.o report.o setup.o
	$Q$(OBJCOPY_FOR_TARGET) $(KEEP_DEBUG) $@ $@.sym
	$Q$(OBJCOPY_FOR_TARGET) $(STRIP_DEBUG) $@
	$Qchmod a-x $@.sym

clean:
	@echo Cleaning benchmarks
	$Q$(RM) stream hg netio RCCE_pingping RCCE_pingpong *.sym *.o *~

veryclean:
	@echo Propper cleaning benchmarks
	$Q$(RM)  stream hg RCCE_pingping RCCE_pingpong *.sym *.o *~

depend:
	$Q$(CC_FOR_TARGET) -MM $(CFLAGS_FOR_TARGET) *.c > Makefile.dep

-include Makefile.dep
# DO NOT DELETE
