set(PACKAGE_VERSION "0.3.1" CACHE STRING
	"HermitCore current version")

set(MAX_ISLE "8" CACHE STRING
	"Maximum number of NUMA isles")

set(MAX_FNAME "128" CACHE STRING
	"Define the maximum length of a file name")

set(KERNEL_STACK_SIZE 32768 CACHE STRING
	"Kernel stack size in bytes")

set(DEFAULT_STACK_SIZE 262144 CACHE STRING
	"Task stack size in bytes")

set(MAX_ARGC_ENVC 128 CACHE STRING
        "Maximum number of command line parameters and enviroment variables
        forwarded to uhyve")
