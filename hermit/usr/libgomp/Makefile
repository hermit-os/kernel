NEWLIB = ../x86/x86_64-hermit
MAKE = make
ARFLAGS_FOR_TARGET = rsv
CP = cp
C_source =  $(wildcard *.c)
NAME = libgomp.a
OBJS = $(C_source:.c=.o)

#
# Prettify output
V = 0
ifeq ($V,0)
	Q = @
	P = > /dev/null
endif

# other implicit rules
%.o : %.c
	@echo [CC] $@
	$Q$(CC_FOR_TARGET) -c $(CFLAGS_FOR_TARGET) -o $@ $<

default: all

all: $(NAME)

$(NAME): $(OBJS)
	$Q$(AR_FOR_TARGET) $(ARFLAGS_FOR_TARGET) $@ $(OBJS)
	$Q$(CP) $@ $(NEWLIB)/lib
	$Q$(CP) libgomp.spec $(NEWLIB)/lib
	$Q$(CP) omp.h $(NEWLIB)/include
	
clean:
	@echo Cleaning examples
	$Q$(RM) $(NAME) *.o *~ 

veryclean:
	@echo Propper cleaning examples
	$Q$(RM) $(NAME) *.o *~

depend:
	$Q$(CC_FOR_TARGET) -MM $(CFLAGS_FOR_TARGET) *.c > Makefile.dep

-include Makefile.dep
# DO NOT DELETE
