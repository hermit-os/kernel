NEWLIB = ../x86/x86_64-hermit
CP = cp
NAME = libxray.a

CC_FOR_TARGET ?= gcc
AR_FOR_TARGET ?= ar

CFLAGS = -DXRAY -DXRAY_DISABLE_BROWSER_INTEGRATION -DXRAY_NO_DEMANGLE -DXRAY_ANNOTATE

CFLAGS += ${CFLAGS_FOR_TARGET}

OBJS = xray.o stringpool.o hashtable.o symtable.o demangle.o parsesymbols.o report.o


$(NAME): $(OBJS)
	$(AR_FOR_TARGET) rsv $@ $(OBJS)
	$(CP) $@ $(NEWLIB)/lib
	$(CP) libxray.spec $(NEWLIB)/lib
	$(CP) xray.h $(NEWLIB)/include

%.o: %.c
	@echo [CC] $@
	@$(CC_FOR_TARGET) $(CFLAGS) -c $< -o $@

clean:
	rm -f *.o $(NAME)
