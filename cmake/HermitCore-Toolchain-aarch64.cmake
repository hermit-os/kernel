include(${CMAKE_CURRENT_LIST_DIR}/HermitCore-Utils.cmake)
include_guard()

# let user provide a different path to the toolchain
set_default(TOOLCHAIN_BIN_DIR /opt/hermit/bin)

set(TARGET_ARCH aarch64)
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
set(CMAKE_C_COMPILER ${TOOLCHAIN_BIN_DIR}/${TARGET_ARCH}-hermit-gcc)
set(CMAKE_CXX_COMPILER ${TOOLCHAIN_BIN_DIR}/${TARGET_ARCH}-hermit-g++)
set(CMAKE_Fortran_COMPILER ${TOOLCHAIN_BIN_DIR}/${TARGET_ARCH}-hermit-gfortran)
set(CMAKE_Go_COMPILER ${TOOLCHAIN_BIN_DIR}/${TARGET_ARCH}-hermit-gccgo)

# hinting the prefix and location is needed in order to correctly detect
# binutils
set(_CMAKE_TOOLCHAIN_PREFIX "${TARGET_ARCH}-hermit-")
set(_CMAKE_TOOLCHAIN_LOCATION ${TOOLCHAIN_BIN_DIR})
