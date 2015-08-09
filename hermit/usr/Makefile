TOPDIR = $(shell pwd)
ARCH = x86
TARGET=x86_64-hermit

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

default: $(ARCH)
	$Q$(MAKE) TARGET=$(TARGET) CFLAGS+="-I. -Iplatform/hermit -Iplatform/helper -ffreestanding -O3 -Wall -I$(NEWLIB)/include -I../../include" LDFLAGS+="-nostdlib -L$(NEWLIB)/lib" -C pte depend
	$Q$(MAKE) TARGET=$(TARGET) CFLAGS+="-I. -Iplatform/hermit -Iplatform/helper -ffreestanding -O3 -Wall -I$(NEWLIB)/include -I../../include" LDFLAGS+="-nostdlib -L$(NEWLIB)/lib" -C pte
	$Q$(MAKE) CFLAGS+="-mtune=native -O3" -C examples depend
	$Q$(MAKE) CFLAGS+="-mtune=native -O3" -C examples

$(ARCH):
	@echo Build newlib
	$Q$(RM) $(TMP)
	$Q$(MKDIR) $(TMP)
	$Q$(MKDIR) $(TMP)/binutils
	$Q$(CD) $(TMP)/binutils; $(TOPDIR)/binutils/configure --target=$(TARGET) --prefix=$(TOPDIR)/$(ARCH) --disable-shared --disable-nls --disable-gdb --disable-libdecnumber --disable-readline --disable-sim --disable-libssp --enable-tls && $(MAKE) && $(MAKE) install
	$Q$(MKDIR) $(TMP)/newlib
	$Q$(CD) $(TMP)/newlib; $(TOPDIR)/src/configure --target=$(TARGET) --prefix=$(TOPDIR)/$(ARCH) $(OPT) && $(MAKE) && $(MAKE) install
	$Q$(MAKE) TARGET=$(TARGET) CFLAGS+="-O3 -I. -Iplatform/hermit -Iplatform/helper -ffreestanding -O3 -Wall -I$(NEWLIB)/include -I../../include -I../../arch/$(ARCH)/include" LDFLAGS+="-nostdlib -L$(NEWLIB)/lib" -C pte depend
	$Q$(MAKE) TARGET=$(TARGET) CFLAGS+="-O3 -I. -Iplatform/hermit -Iplatform/helper -ffreestanding -O3 -Wall -I$(NEWLIB)/include -I../../include -I../../arch/$(ARCH)/include" LDFLAGS+="-nostdlib -L$(NEWLIB)/lib" -C pte
	$Q$(MKDIR) $(TMP)/gcc
	$Q$(CD) $(TMP)/gcc; $(TOPDIR)/gcc/configure --target=$(TARGET) --prefix=$(TOPDIR)/$(ARCH) --without-headers --with-newlib --enable-languages=c --disable-nls --disable-shared --disable-libssp --enable-threads=posix --enable-tls && $(MAKE) && $(MAKE) install

clean:
	@echo Cleaning newlib
	$Q$(MAKE) -C examples clean
	$Q$(RM) $(TMP)

veryclean:
	@echo Propper cleaning newlib
	$Q$(MAKE) -C pte veryclean
	$Q$(MAKE) -C examples veryclean
	$Q$(RM) $(TOPDIR)/$(ARCH)
