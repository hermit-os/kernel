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

demo:
	$Q$(MAKE) CC_FOR_TARGET=$(CC_FOR_TARGET) CFLAGS_FOR_TARGET="$(CFLAGS_FOR_TARGET)" LDFLAGS_FOR_TARGET="$(LDFLAGS_FOR_TARGET)" OBJCOPY_FOR_TARGET=$(OBJCOPY_FOR_TARGET) -C examples depend
	$Q$(MAKE) CC_FOR_TARGET=$(CC_FOR_TARGET) CFLAGS_FOR_TARGET="$(CFLAGS_FOR_TARGET)" LDFLAGS_FOR_TARGET="$(LDFLAGS_FOR_TARGET)" OBJCOPY_FOR_TARGET=$(OBJCOPY_FOR_TARGET) -C examples

$(ARCH):
	$Q$(MKDIR) $(TMP)

bootstrap: $(ARCH) $(TMP)/binutils $(TMP)/bootstrap

$(TMP)/binutils:
	$Q$(MKDIR) $(TMP)/binutils
	$Q$(CD) $(TMP)/binutils; $(TOPDIR)/binutils/configure --target=$(TARGET) --prefix=$(TOPDIR)/$(ARCH) --disable-shared --disable-nls --disable-gdb --disable-libdecnumber --disable-readline --disable-sim --disable-libssp --enable-tls && $(MAKE) $(NJOBS) && $(MAKE) install


$(TMP)/bootstrap:
	$Q$(MKDIR) $(TMP)/bootstrap
	$Q$(CD) $(TMP)/bootstrap; $(TOPDIR)/gcc/configure --target=$(TARGET) --prefix=$(TOPDIR)/$(ARCH) --without-headers --enable-languages=c --disable-nls --disable-shared --disable-libssp --enable-threads=posix --enable-tls && $(MAKE) $(NJOBS) all-gcc && $(MAKE) install-gcc

toolchain: $(TMP)/newlib $(TMP)/gcc demo

$(TMP)/newlib:
	$Q$(MKDIR) $(TMP)/newlib
	$Q$(CD) $(TMP)/newlib; $(TOPDIR)/newlib/configure --target=$(TARGET) --prefix=$(TOPDIR)/$(ARCH) $(OPT) && $(MAKE) $(NJOBS) && $(MAKE) install
	$Q$(MAKE) TARGET=$(TARGET) CC_FOR_TARGET=$(CC_FOR_TARGET) AR_FOR_TARGET=$(AR_FOR_TARGET) CFLAGS_FOR_TARGET+="-O3 -I. -Iplatform/hermit -Iplatform/helper -Wall" -C pte depend
	$Q$(MAKE) TARGET=$(TARGET) CC_FOR_TARGET=$(CC_FOR_TARGET) AR_FOR_TARGET=$(AR_FOR_TARGET) CFLAGS_FOR_TARGET+="-O3 -I. -Iplatform/hermit -Iplatform/helper -Wall" -C pte

$(TMP)/gcc:
	$Q$(MKDIR) $(TMP)/gcc
	$Q$(CD) $(TMP)/gcc; $(TOPDIR)/gcc/configure --target=$(TARGET) --prefix=$(TOPDIR)/$(ARCH) --without-headers --with-newlib --enable-languages=c --disable-nls --disable-shared --disable-libssp --enable-threads=posix --enable-tls && $(MAKE) $(NJOBS) && $(MAKE) install

clean:
	@echo Cleaning toolchain
	$Q$(MAKE) -C examples clean
	$Q$(RM) $(TMP)

veryclean:
	@echo Propper cleaning of the toolchain
	$Q$(MAKE) -C pte veryclean
	$Q$(MAKE) -C examples veryclean
	$Q$(RM) $(TOPDIR)/$(ARCH)
