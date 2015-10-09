override TOPDIR = $(shell pwd)
ARCH = x86
TARGET=x86_64-hermit
NJOBS=-j20

NEWLIB = $(TOPDIR)/$(ARCH)/$(TARGET)
RM = rm -rf
CD = cd
MKDIR = mkdir
TMP = $(TOPDIR)/tmp
OPT = --disable-shared --disable-multilib --enable-newlib-io-c99-formats --enable-newlib-multithread #--enable-newlib-reent-small

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
	$Q$(MAKE) CC_FOR_TARGET=$(CC_FOR_TARGET) CXX_FOR_TARGET=$(CXX_FOR_TARGET) CXXFLAGS_FOR_TARGET="$(CXXFLAGS_FOR_TARGET)" CFLAGS_FOR_TARGET="$(CFLAGS_FOR_TARGET)" LDFLAGS_FOR_TARGET="$(LDFLAGS_FOR_TARGET)" OBJCOPY_FOR_TARGET=$(OBJCOPY_FOR_TARGET) -C tests depend
	$Q$(MAKE) CC_FOR_TARGET=$(CC_FOR_TARGET) CXX_FOR_TARGET=$(CXX_FOR_TARGET) CXXFLAGS_FOR_TARGET="$(CXXFLAGS_FOR_TARGET)" CFLAGS_FOR_TARGET="$(CFLAGS_FOR_TARGET)" LDFLAGS_FOR_TARGET="$(LDFLAGS_FOR_TARGET)" OBJCOPY_FOR_TARGET=$(OBJCOPY_FOR_TARGET) -C tests

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
	$Q$(CD) $(TMP)/bootstrap; $(TOPDIR)/gcc/configure --target=$(TARGET) --prefix=$(TOPDIR)/$(ARCH) --without-headers --enable-languages=c --disable-nls --disable-shared --disable-libssp --disable-libgomp --enable-threads=posix --enable-tls && $(MAKE) $(NJOBS) all-gcc && $(MAKE) install-gcc

toolchain: $(TMP)/newlib libs $(TMP)/gcc demo

$(TMP)/newlib:
	@echo Build newlib, libpthread and libgomp
	$Q$(MKDIR) $(TMP)/newlib
	$Q$(CD) $(TMP)/newlib; $(TOPDIR)/newlib/configure --target=$(TARGET) --prefix=$(TOPDIR)/$(ARCH) $(OPT) && $(MAKE) $(NJOBS) && $(MAKE) install

libs:
	$Q$(MAKE) TARGET=$(TARGET) CC_FOR_TARGET=$(CC_FOR_TARGET) AR_FOR_TARGET=$(AR_FOR_TARGET) CFLAGS_FOR_TARGET+="-I. -Iplatform/hermit -Iplatform/helper -Wall" -C pte depend
	$Q$(MAKE) TARGET=$(TARGET) CC_FOR_TARGET=$(CC_FOR_TARGET) AR_FOR_TARGET=$(AR_FOR_TARGET) CFLAGS_FOR_TARGET+="-I. -Iplatform/hermit -Iplatform/helper -Wall" -C pte
	$Q$(MAKE) TARGET=$(TARGET) CC_FOR_TARGET=$(CC_FOR_TARGET) AR_FOR_TARGET=$(AR_FOR_TARGET) CFLAGS_FOR_TARGET+="-I. -Wall -pthread" -C libgomp depend
	$Q$(MAKE) TARGET=$(TARGET) CC_FOR_TARGET=$(CC_FOR_TARGET) AR_FOR_TARGET=$(AR_FOR_TARGET) CFLAGS_FOR_TARGET+="-I. -Wall -pthread" -C libgomp

$(TMP)/gcc:
	@echo Build final gcc
	$Q$(MKDIR) $(TMP)/gcc
	$Q$(CD) $(TMP)/gcc; $(TOPDIR)/gcc/configure --target=$(TARGET) --prefix=$(TOPDIR)/$(ARCH) --without-headers --with-newlib --enable-languages=c --disable-nls --disable-shared --disable-libssp --enable-threads=posix --disable-libgomp --enable-tls --enable-lto --disable-symvers && $(MAKE) $(NJOBS) && $(MAKE) install
	$Q$(RM) $(TMP)/gcc
	$Q$(MKDIR) $(TMP)/gcc
	$Q$(CD) $(TMP)/gcc; $(TOPDIR)/gcc/configure --target=$(TARGET) --prefix=$(TOPDIR)/$(ARCH) --without-headers --with-newlib --enable-languages=c,c++,fortran,lto --disable-nls --disable-shared --disable-libssp --enable-threads=posix --disable-libgomp --enable-tls --enable-lto --disable-symvers && $(MAKE) $(NJOBS) && $(MAKE) install

clean:
	@echo Cleaning toolchain
	$Q$(MAKE) -C tests clean

veryclean:
	@echo Propper cleaning of the toolchain
	$Q$(MAKE) -C pte veryclean
	$Q$(MAKE) -C libgomp veryclean
	$Q$(MAKE) -C tests veryclean
	$Q$(RM) $(TOPDIR)/$(ARCH)
	$Q$(RM) $(TMP)
