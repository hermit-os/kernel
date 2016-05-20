override TOPDIR = $(shell pwd)
ARCH = x86
TARGET=x86_64-hermit
NJOBS=-j$(shell nproc)
#OMPRT=libgomp
OMPRT=libomp

CROSSCOMPREFIX = x86_64-hermit
NEWLIB = $(TOPDIR)/$(ARCH)/$(TARGET)
RM = rm -rf
CD = cd
CP = cp
MKDIR = mkdir -p
TMP = $(TOPDIR)/tmp
OPT = --disable-shared --disable-multilib --enable-lto --enable-newlib-hw-fp --enable-newlib-io-c99-formats --enable-newlib-multithread #--enable-newlib-reent-small

# Prettify output
V = 0
ifeq ($V,0)
	Q = @
	P = > /dev/null
endif

default:
	@echo Do not use default rule!

demo:
	@echo Build demo applications
	$Q$(MAKE) ELFEDIT_FOR_TARGET=$(ELFEDIT_FOR_TARGET) CC_FOR_TARGET=$(CC_FOR_TARGET) CXX_FOR_TARGET=$(CXX_FOR_TARGET) CXXFLAGS_FOR_TARGET="$(CXXFLAGS_FOR_TARGET)" CFLAGS_FOR_TARGET="$(CFLAGS_FOR_TARGET) $(PROFILING_CFLAGS)" LDFLAGS_FOR_TARGET="$(LDFLAGS_FOR_TARGET) $(PROFILING_LDFLAGS)" OBJCOPY_FOR_TARGET=$(OBJCOPY_FOR_TARGET) -C tests depend
	$Q$(MAKE) ELFEDIT_FOR_TARGET=$(ELFEDIT_FOR_TARGET) CC_FOR_TARGET=$(CC_FOR_TARGET) CXX_FOR_TARGET=$(CXX_FOR_TARGET) CXXFLAGS_FOR_TARGET="$(CXXFLAGS_FOR_TARGET)" CFLAGS_FOR_TARGET="$(CFLAGS_FOR_TARGET) $(PROFILING_CFLAGS)" LDFLAGS_FOR_TARGET="$(LDFLAGS_FOR_TARGET) $(PROFILING_LDFLAGS)" OBJCOPY_FOR_TARGET=$(OBJCOPY_FOR_TARGET) -C tests
	$Q$(MAKE) ELFEDIT_FOR_TARGET=$(ELFEDIT_FOR_TARGET) CC_FOR_TARGET=$(CC_FOR_TARGET) CXX_FOR_TARGET=$(CXX_FOR_TARGET) CXXFLAGS_FOR_TARGET="$(CXXFLAGS_FOR_TARGET)" CFLAGS_FOR_TARGET="$(CFLAGS_FOR_TARGET) $(PROFILING_CFLAGS)" LDFLAGS_FOR_TARGET="$(LDFLAGS_FOR_TARGET) $(PROFILING_LDFLAGS)" OBJCOPY_FOR_TARGET=$(OBJCOPY_FOR_TARGET) -C benchmarks depend
	$Q$(MAKE) ELFEDIT_FOR_TARGET=$(ELFEDIT_FOR_TARGET) CC_FOR_TARGET=$(CC_FOR_TARGET) CXX_FOR_TARGET=$(CXX_FOR_TARGET) CXXFLAGS_FOR_TARGET="$(CXXFLAGS_FOR_TARGET)" CFLAGS_FOR_TARGET="$(CFLAGS_FOR_TARGET) $(PROFILING_CFLAGS)" LDFLAGS_FOR_TARGET="$(LDFLAGS_FOR_TARGET) $(PROFILING_LDFLAGS)" OBJCOPY_FOR_TARGET=$(OBJCOPY_FOR_TARGET) -C benchmarks
	#$Q$(MAKE) ELFEDIT_FOR_TARGET=$(ELFEDIT_FOR_TARGET) CC_FOR_TARGET=$(CC_FOR_TARGET) CXX_FOR_TARGET=$(CXX_FOR_TARGET) CXXFLAGS_FOR_TARGET="$(CXXFLAGS_FOR_TARGET)" CFLAGS_FOR_TARGET="$(CFLAGS_FOR_TARGET)" LDFLAGS_FOR_TARGET="$(LDFLAGS_FOR_TARGET)" OBJCOPY_FOR_TARGET=$(OBJCOPY_FOR_TARGET) -C openmpbench depend
	$Q$(MAKE) ELFEDIT_FOR_TARGET=$(ELFEDIT_FOR_TARGET) CC_FOR_TARGET=$(CC_FOR_TARGET) CXX_FOR_TARGET=$(CXX_FOR_TARGET) CXXFLAGS_FOR_TARGET="$(CXXFLAGS_FOR_TARGET)" CFLAGS_FOR_TARGET="$(CFLAGS_FOR_TARGET) $(PROFILING_CFLAGS)" LDFLAGS_FOR_TARGET="$(LDFLAGS_FOR_TARGET) $(PROFILING_LDFLAGS)" OBJCOPY_FOR_TARGET=$(OBJCOPY_FOR_TARGET) -C openmpbench

$(ARCH):
	$Q$(MKDIR) $(TMP)

bootstrap: $(ARCH) $(TMP)/binutils $(TMP)/bootstrap

$(TMP)/binutils:
	@echo Build binutils
	$Q$(MKDIR) $(TMP)/binutils
	$Q$(CD) $(TMP)/binutils; $(TOPDIR)/binutils/configure --target=$(TARGET) --prefix=$(TOPDIR)/$(ARCH) --disable-shared --disable-nls --disable-gdb --disable-libdecnumber --disable-readline --disable-sim --disable-libssp --enable-tls && $(MAKE) $(NJOBS) && $(MAKE) install


$(TMP)/bootstrap:
	@echo Build bootstrap compiler
	$Q$(MKDIR) $(TMP)/bootstrap
	$Q$(CD) $(TMP)/bootstrap; $(TOPDIR)/gcc/configure --target=$(TARGET) --prefix=$(TOPDIR)/$(ARCH) --without-headers --with-isl --with-tune=generic --enable-languages=c --disable-nls --disable-shared --disable-libssp --disable-libgomp --enable-threads=posix --enable-tls && $(MAKE) $(NJOBS) all-gcc $(P) && $(MAKE) install-gcc $(P)

toolchain: $(TMP)/newlib pte $(TMP)/gcc libs headers demo

$(TMP)/newlib:
	@echo Build newlib
	$Q$(MKDIR) $(TMP)/newlib
	$Q$(CD) $(TMP)/newlib; $(TOPDIR)/newlib/configure --target=$(TARGET) --prefix=$(TOPDIR)/$(ARCH) $(OPT) && $(MAKE) $(NJOBS) && $(MAKE) install

pte:
	@echo Build libpthread
	$Q$(MAKE) TARGET=$(TARGET) CC_FOR_TARGET=$(CC_FOR_TARGET) AR_FOR_TARGET=$(AR_FOR_TARGET) CFLAGS_FOR_TARGET+="-I. -Iplatform/hermit -Iplatform/helper -Wall $(PROFILING_CFLAGS)" -C pte depend
	$Q$(MAKE) TARGET=$(TARGET) CC_FOR_TARGET=$(CC_FOR_TARGET) AR_FOR_TARGET=$(AR_FOR_TARGET) CFLAGS_FOR_TARGET+="-I. -Iplatform/hermit -Iplatform/helper -Wall $(PROFILING_CFLAGS)" -C pte

libs:
	@echo Build Xray profiler
	$Q$(MAKE) TARGET=$(TARGET) CC_FOR_TARGET=$(CC_FOR_TARGET) CFLAGS_FOR_TARGET="$(CFLAGS_FOR_TARGET)" LDFLAGS_FOR_TARGET="$(LDFLAGS_FOR_TARGET)" -C xray
	@echo Build OpenMP Runtime and iRCCE
	$Q$(MAKE) TARGET=$(TARGET) CXX_FOR_TARGET=$(CXX_FOR_TARGET) CXXFLAGS_FOR_TARGET="$(CXXFLAGS_FOR_TARGET)" CC_FOR_TARGET=$(CC_FOR_TARGET) AR_FOR_TARGET=$(AR_FOR_TARGET) CFLAGS_FOR_TARGET+="-I. -Wall -pthread $(PROFILING_CFLAGS)" -C $(OMPRT) depend
	$Q$(MAKE) TARGET=$(TARGET) CXX_FOR_TARGET=$(CXX_FOR_TARGET) CXXFLAGS_FOR_TARGET="$(CXXFLAGS_FOR_TARGET)" CC_FOR_TARGET=$(CC_FOR_TARGET) AR_FOR_TARGET=$(AR_FOR_TARGET) CFLAGS_FOR_TARGET+="-I. -Wall -pthread $(PROFILING_CFLAGS)" -C $(OMPRT)
	$Q$(MAKE) TARGET=$(TARGET) CC_FOR_TARGET=$(CC_FOR_TARGET) AR_FOR_TARGET=$(AR_FOR_TARGET) CFLAGS_FOR_TARGET+="-I. -Wall" -C ircce depend
	$Q$(MAKE) TARGET=$(TARGET) CC_FOR_TARGET=$(CC_FOR_TARGET) AR_FOR_TARGET=$(AR_FOR_TARGET) CFLAGS_FOR_TARGET+="-I. -Wall" -C ircce

headers:
	@echo [CP] Copy headers
	$Q$(CP) -R ../lwip/src/include/posix/* $(TOPDIR)/$(ARCH)/$(CROSSCOMPREFIX)/include/
	$Q$(CP) -R ../lwip/src/include/arch $(TOPDIR)/$(ARCH)/$(CROSSCOMPREFIX)/include/
	$Q$(CP) -R ../lwip/src/include/lwip* $(TOPDIR)/$(ARCH)/$(CROSSCOMPREFIX)/include/
	$Q$(CP) -R ../lwip/src/include/ipv4/lwip/* $(TOPDIR)/$(ARCH)/$(CROSSCOMPREFIX)/include/lwip/
	$Q$(CP) -R ../include/neti* $(TOPDIR)/$(ARCH)/$(CROSSCOMPREFIX)/include/
	$Q$(CP) -R ../include/sys $(TOPDIR)/$(ARCH)/$(CROSSCOMPREFIX)/include/

$(TMP)/gcc:
	@echo Build final gcc
	$Q$(MKDIR) $(TMP)/gcc
	$Q$(CD) $(TMP)/gcc; $(TOPDIR)/gcc/configure --target=$(TARGET) --prefix=$(TOPDIR)/$(ARCH) --without-headers --with-newlib --with-isl --with-tune=generic --enable-languages=c --disable-nls --disable-shared --disable-libssp --enable-threads=posix --disable-libgomp --enable-tls --enable-lto --disable-symvers && $(MAKE) $(NJOBS) $(P) && $(MAKE) install $(P)
	$Q$(RM) $(TMP)/gcc
	$Q$(MKDIR) $(TMP)/gcc
	$Q$(CD) $(TMP)/gcc; $(TOPDIR)/gcc/configure --target=$(TARGET) --prefix=$(TOPDIR)/$(ARCH) --without-headers --with-newlib --with-isl --with-tune=generic --enable-languages=c,c++,fortran,lto --disable-nls --disable-shared --disable-libssp --enable-threads=posix --disable-libgomp --enable-tls --enable-lto --disable-symvers && $(MAKE) $(NJOBS) $(P) && $(MAKE) install $(P)

clean:
	@echo Cleaning toolchain
	$Q$(MAKE) -C pte clean
	$Q$(MAKE) -C $(OMPRT) clean
	$Q$(MAKE) -C ircce clean
	$Q$(MAKE) -C tests clean
	$Q$(MAKE) -C benchmarks clean
	$Q$(MAKE) -C openmpbench clean
	$Q$(MAKE) -C xray clean

veryclean:
	@echo Propper cleaning of the toolchain
	$Q$(MAKE) -C pte veryclean
	$Q$(MAKE) -C $(OMPRT) veryclean
	$Q$(MAKE) -C ircce veryclean
	$Q$(MAKE) -C tests veryclean
	$Q$(MAKE) -C benchmarks veryclean
	$Q$(MAKE) -C openmpbench veryclean
	$Q$(RM) $(TOPDIR)/$(ARCH)
	$Q$(RM) $(TMP)

.PHONY: default all clean veryclean headers pte libs
