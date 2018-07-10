include(${CMAKE_CURRENT_LIST_DIR}/HermitCore-Utils.cmake)
include_guard()

# let user provide a different path to the toolchain
set_default(TOOLCHAIN_BIN_DIR /opt/hermit/bin)

set(TARGET_ARCH x86_64)
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
					
set(CMAKE_SYSTEM_NAME Generic)

# point CMake to our toolchain
# In Debug mode, the Rust-compiled libhermit.a contains references to non-existing software floating-point functions (like __floatundisf).
# We have to remove these with a linker flag as early as possible.
set(GC_SECTIONS_FLAG "-Wl,--gc-sections")
set(CMAKE_C_COMPILER ${TOOLCHAIN_BIN_DIR}/${TARGET_ARCH}-hermit-gcc ${GC_SECTIONS_FLAG})
set(CMAKE_CXX_COMPILER ${TOOLCHAIN_BIN_DIR}/${TARGET_ARCH}-hermit-g++ ${GC_SECTIONS_FLAG})
set(CMAKE_Fortran_COMPILER ${TOOLCHAIN_BIN_DIR}/${TARGET_ARCH}-hermit-gfortran ${GC_SECTIONS_FLAG})
set(CMAKE_Go_COMPILER "${TOOLCHAIN_BIN_DIR}/${TARGET_ARCH}-hermit-gccgo" "${GC_SECTIONS_FLAG}")

# hinting the prefix and location is needed in order to correctly detect
# binutils
set(_CMAKE_TOOLCHAIN_PREFIX "${TARGET_ARCH}-hermit-")
set(_CMAKE_TOOLCHAIN_LOCATION ${TOOLCHAIN_BIN_DIR})
