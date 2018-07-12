include(${CMAKE_CURRENT_LIST_DIR}/HermitCore-Utils.cmake)
include_guard()

# let user provide a different path to the toolchain
set_default(TOOLCHAIN_BIN_DIR /opt/hermit/bin)

set(HERMIT_ARCH aarch64)
set(HERMIT_KERNEL_FLAGS
					-Wall -O2 -mgeneral-regs-only
					-fno-var-tracking-assignments -fstrength-reduce
					-fomit-frame-pointer -finline-functions -ffreestanding
					-nostdinc -fno-stack-protector
					-fno-delete-null-pointer-checks
					-falign-jumps=1 -falign-loops=1
					-fno-common -Wframe-larger-than=1024
					-fno-strict-aliasing -fno-asynchronous-unwind-tables
					-fno-strict-overflow)

set(HERMIT_APP_FLAGS
					-O3 -ftree-vectorize)

set(CMAKE_SYSTEM_NAME Generic)

# point CMake to our toolchain
set(CMAKE_C_COMPILER ${TOOLCHAIN_BIN_DIR}/${HERMIT_ARCH}-hermit-gcc)
set(CMAKE_CXX_COMPILER ${TOOLCHAIN_BIN_DIR}/${HERMIT_ARCH}-hermit-g++)
set(CMAKE_Fortran_COMPILER ${TOOLCHAIN_BIN_DIR}/${HERMIT_ARCH}-hermit-gfortran)
set(CMAKE_Go_COMPILER ${TOOLCHAIN_BIN_DIR}/${HERMIT_ARCH}-hermit-gccgo)

# Building a HermitCore application won't work before HermitCore is installed in /opt/hermit because of the missing libhermit.a
# So only try to compile a static library for compiler testing.
set(CMAKE_TRY_COMPILE_TARGET_TYPE STATIC_LIBRARY)

# hinting the prefix and location is needed in order to correctly detect
# binutils
set(_CMAKE_TOOLCHAIN_PREFIX "${HERMIT_ARCH}-hermit-")
set(_CMAKE_TOOLCHAIN_LOCATION ${TOOLCHAIN_BIN_DIR})
