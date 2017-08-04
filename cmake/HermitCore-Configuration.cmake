set(PACKAGE_VERSION "0.2.2" CACHE STRING
	"HermitCore current version")

set(MAX_CORES "512" CACHE STRING
	"Maximum number of cores that can be managed")

set(MAX_TASKS "((MAX_CORES * 2) + 2)" CACHE STRING
	"Maximum number of tasks")

set(MAX_ISLE "8" CACHE STRING
	"Maximum number of NUMA isles")

set(MAX_FNAME "128" CACHE STRING
	"Define the maximum length of a file name")

set(KERNEL_STACK_SIZE 8192 CACHE STRING
	"Kernel stack size in bytes")

set(DEFAULT_STACK_SIZE 262144 CACHE STRING
	"Task stack size in bytes")

option(DYNAMIC_TICKS
	"Don't use a periodic timer event to keep track of time" ON)

option(SAVE_FPU
	"Save FPU registers on context switch" ON)

option(HAVE_ARCH_MEMSET	 "Use machine specific version of memset"  OFF)
option(HAVE_ARCH_MEMCPY	 "Use machine specific version of memcpy"  OFF)
option(HAVE_ARCH_STRLEN	 "Use machine specific version of strlen"  OFF)
option(HAVE_ARCH_STRCPY	 "Use machine specific version of strcpy"  OFF)
option(HAVE_ARCH_STRNCPY "Use machine specific version of strncpy" OFF)
