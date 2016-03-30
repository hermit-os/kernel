include Makefile.defs

# We patch the ELF header with a custom OSABI to be recognized by binfmt_misc
ELF_OSABI_OFFSET = 7
ELF_OSABI = "\\x42"

# If CFLAGS_CRAY is empty set it to CFLAGS
ifeq ($(CFLAGS_CRAY),)
CFLAGS_CRAY = ${CFLAGS}
endif

%.bin: %
	$(OBJCOPY_FOR_TARGET) $(OBJCOPY_FLAGS) $< $@

.c.o:
	${CC} ${CFLAGS} $(OMPFLAG) -c $*.c 

SYNCOBJS =  syncbench.o common.o 
SCHEDOBJS = schedbench.o common_sched.o 
ARRAYOBJS = arraybench_$(IDA).o common.o 
TASKOBJS =  taskbench.o common.o 
SCHEDFLAGS = -DSCHEDBENCH

all:	syncbench.bin schedbench.bin taskbench.bin

prog: arraybench_$(IDA) 

syncbench: $(SYNCOBJS)
	$(CC) -o syncbench $(LDFLAGS) $(SYNCOBJS) $(CLOCKOBJS) $(LIBS) -lm
	$Qprintf $(ELF_OSABI) | dd of=$@ bs=1 seek=$(ELF_OSABI_OFFSET) count=1 conv=notrunc 2>/dev/null

# Rule to ensure the lower optimisation level is picked up for common.c 
# with the Cray compiler
common.o:	
	${CC} ${CFLAGS_CRAY} $(OMPFLAG) -c $*.c 

# Separate rule to build common_sched.o as we need to ensure the correct 
# DEFAULT_DELAY_TIME is used. 
common_sched.o:	
	${CC} ${CFLAGS_CRAY} $(SCHEDFLAGS) $(OMPFLAG) -o common_sched.o -c common.c

schedbench: $(SCHEDOBJS)
	$(CC) -o schedbench  $(LDFLAGS) $(SCHEDOBJS) $(CLOCKOBJS) $(LIBS) -lm
	$Qprintf $(ELF_OSABI) | dd of=$@ bs=1 seek=$(ELF_OSABI_OFFSET) count=1 conv=notrunc 2>/dev/null

# Multiple header files due to multiple array sizes, makes header file arraybench_*.h
arraybench_$(IDA).h: arraybench.h
	$(CPP) -DIDA=$(IDA) $(OMPFLAG) -P arraybench.h -o $@

# Multiple object files due to multiple array sizes, makes object file arraybench_*.o
arraybench_$(IDA).o: arraybench_$(IDA).h arraybench.c
	$(CC) $(CFLAGS) -DIDA=$(IDA) $(OMPFLAG) arraybench.c -c -o $@

# Multiple executables due to multiple array sizes, makes exe file arraybench_*
arraybench_$(IDA): $(ARRAYOBJS) $(CLOCKOBJS) arraybench.c
	$(CC) $(LDFLAGS) $(ARRAYOBJS) $(CLOCKOBJS) $(LIBS) -lm -o $@ 

taskbench: $(TASKOBJS)
	$(CC) -o taskbench $(LDFLAGS) $(OMPFLAG) $(TASKOBJS) $(CLOCKOBJS) $(LIBS) -lm 
	$Qprintf $(ELF_OSABI) | dd of=$@ bs=1 seek=$(ELF_OSABI_OFFSET) count=1 conv=notrunc 2>/dev/null

clean: 
	-rm -rf *.o syncbench schedbench arraybench_* taskbench

veryclean: clean
	-rm -rf OpenMPBench.* *.all
