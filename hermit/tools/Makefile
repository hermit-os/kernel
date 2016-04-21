MAKE = make
CC = gcc
CFLAGS = -O2 -Wall
PROXYFILES = proxy init.sh $(shell find ../usr/tests ../usr/benchmarks ../usr/openmpbench -type f -executable)

# Prettify output
V = 0
ifeq ($V,0)
	Q = @
	P = > /dev/null
endif

# other implicit rules
%.o : %.c
	@echo [CC] $@
	$Q$(CC) -c $(CFLAGS) -o $@ $<

all: proxy

proxy: proxy.o
	@echo [LD] $@
	$Q$(CC) $(CFLAGS) -o $@ $< -lelf

clean:
	@echo Cleaning tools
	$Q$(RM) -rf *.o *~ *.bin *.obj

veryclean: clean

depend:
	$(CC) -MM $(CFLAGS) *.c > Makefile.dep

-include Makefile.dep
# DO NOT DELETE
