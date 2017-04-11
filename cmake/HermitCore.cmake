include(${CMAKE_CURRENT_LIST_DIR}/HermitCore-Utils.cmake)
include_guard()

include(${CMAKE_CURRENT_LIST_DIR}/HermitCore-Paths.cmake)
include(${CMAKE_CURRENT_LIST_DIR}/HermitCore-Configuration.cmake)

# scripts to detect HermitCore Go compiler
list(APPEND CMAKE_MODULE_PATH ${CMAKE_CURRENT_LIST_DIR}/golang/)

if(NOT HERMIT_ARCH)
	set(HERMIT_ARCH x86)
endif()

if(PROFILING)
	# link everything against XRay
	link_libraries(-lxray)

	# generate symbol map file for XRay to resolve function names
	link_libraries(-Wl,-Map=$<TARGET_PROPERTY:NAME>.map)

	# enable profiling with XRay
	add_compile_options(-falign-functions=32 -finstrument-functions
		-finstrument-functions-exclude-function-list=_mm_pause,_mm_setcsr,_mm_getcsr)
	add_definitions(-DXRAY -DXRAY_DISABLE_BROWSER_INTEGRATION
					-DXRAY_NO_DEMANGLE -DXRAY_ANNOTATE)
endif()

# use default toolchain if not specified by user
if(NOT CMAKE_TOOLCHAIN_FILE)
	if(BOOTSTRAP)
		# use bootstrap toolchain if requested
		set(_BOOTSTRAP_ARCH_SUFFIX -bootstrap)
	endif()
	set(CMAKE_TOOLCHAIN_FILE ${CMAKE_CURRENT_LIST_DIR}/HermitCore-Toolchain-${HERMIT_ARCH}${_BOOTSTRAP_ARCH_SUFFIX}.cmake)
endif()

# NASM detection will change binary format depending on host system, but
# we only want to generate elf64 for HermitCore
# Note: Has to be set *before* ASM_NASM is enabled
set(CMAKE_ASM_NASM_OBJECT_FORMAT elf64)

enable_language(ASM_NASM)

# NASM hack, because it requires include paths to have a trailing /, whereas
# CMake explicitly will remove it when adding includes the usual way
# Note: Has to be set *after* ASM_NASM is enabled
set(CMAKE_ASM_NASM_FLAGS
	"${CMAKE_ASM_NASM_FLAGS} -I ${CMAKE_BINARY_DIR}/include/")

set(HERMIT_KERNEL_FLAGS
					-m64 -Wall -O2 -mno-red-zone
					-fno-var-tracking-assignments -fstrength-reduce
					-fomit-frame-pointer -finline-functions -ffreestanding
					-nostdinc -fno-stack-protector -mno-sse -mno-mmx
					-mno-sse2 -mno-3dnow -mno-avx
					-fno-delete-null-pointer-checks
					-falign-jumps=1 -falign-loops=1
					-mno-80387 -mno-fp-ret-in-387 -mskip-rax-setup
					-fno-common -Wframe-larger-than=1024
					-fno-strict-aliasing -fno-asynchronous-unwind-tables
					-fno-strict-overflow -maccumulate-outgoing-args)

set(HERMIT_APP_FLAGS
					-m64 -mtls-direct-seg-refs -O3 -ftree-vectorize)

if(MTUNE)
	set(HERMIT_KERNEL_FLAGS ${HERMIT_KERNEL_FLAGS} -mtune=${MTUNE})
	set(HERMIT_APP_FLAGS    ${HERMIT_APP_FLAGS}    -mtune=${MTUNE})
endif()

set(HERMIT_KERNEL_INCLUDES
    ${CMAKE_BINARY_DIR}/include
    ${HERMIT_ROOT}/include
    ${HERMIT_ROOT}/arch/${HERMIT_ARCH}/include
    ${HERMIT_ROOT}/lwip/src/include
    ${HERMIT_ROOT}/drivers)

# HACK: when CMake detects compilers it taints CMAKE_INSTALL_PREFIX, so in
#       order to rely on that variable (we redefine it), enable all languages
#       here and source pathes again.
#
# Furthermore this will produce a sensible error message if the toolchain cannot
# be found.
if(NOT BOOTSTRAP)
	enable_language(C CXX Fortran Go)
	include(${CMAKE_CURRENT_LIST_DIR}/HermitCore-Paths.cmake)
endif()

# find elfedit, CMake doesn't use this program, so we have to find it ourself
find_toolchain_program(elfedit)
