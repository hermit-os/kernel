TOPDIR = $(shell pwd)
ARCH = x86
TARGET=x86_64-hermit-elf64

NEWLIB = $(TOPDIR)/$(ARCH)/$(TARGET)
RM = rm -rf
CD = cd
MKDIR = mkdir
TMP = $(TOPDIR)/tmp
OPT = --disable-shared --disable-multilib --disable-newlib-multithread --disable-newlib-reent-small

# Prettify output
V = 0
ifeq ($V,0)
	Q = @
	P = > /dev/null
endif

default: $(ARCH)
	$Q$(MAKE) ARCH=$(ARCH) TARGET=$(TARGET) CFLAGS+="-ffreestanding -Wall -I$(NEWLIB)/include -I../../include -I../../arch/$(ARCH)/include" LDFLAGS+="-nostdlib -L$(NEWLIB)/lib" -C examples depend
	$Q$(MAKE) ARCH=$(ARCH) TARGET=$(TARGET) CFLAGS+="-ffreestanding -Wall -I$(NEWLIB)/include -I../../include -I../../arch/$(ARCH)/include" LDFLAGS+="-nostdlib -L$(NEWLIB)/lib" -C examples

$(ARCH):
	@echo Build newlib
	$Q$(RM) $(TMP)
	$Q$(MKDIR) $(TMP)
	$Q$(CD) $(TMP); $(TOPDIR)/src/configure --target=$(TARGET) --prefix=$(TOPDIR)/$(ARCH) $(OPT) && $(MAKE) && $(MAKE) install

clean:
	@echo Cleaning newlib
	$Q$(MAKE) -C examples clean
	$Q$(RM) $(TMP)

veryclean:
	@echo Propper cleaning newlib
	$Q$(MAKE) -C examples veryclean
	$Q$(RM) $(TOPDIR)/$(ARCH)
