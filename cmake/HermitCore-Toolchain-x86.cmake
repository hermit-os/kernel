include(${CMAKE_CURRENT_LIST_DIR}/HermitCore-Utils.cmake)
include_guard()

# let user provide a different path to the toolchain
set_default(TOOLCHAIN_BIN_DIR /opt/hermit/bin)

set(TARGET_ARCH x86_64-hermit)

set(CMAKE_SYSTEM_NAME Generic)

# Point CMake to our toolchain
# In Debug mode, the Rust-compiled libhermit.a contains references to non-existing software floating-point functions (like __floatundisf).
# We have to remove these with a linker flag as early as possible.
set(GC_SECTIONS_FLAG "-Wl,--gc-sections")
set(CMAKE_C_COMPILER ${TOOLCHAIN_BIN_DIR}/${TARGET_ARCH}-gcc ${GC_SECTIONS_FLAG})
set(CMAKE_CXX_COMPILER ${TOOLCHAIN_BIN_DIR}/${TARGET_ARCH}-g++ ${GC_SECTIONS_FLAG})
set(CMAKE_Fortran_COMPILER ${TOOLCHAIN_BIN_DIR}/${TARGET_ARCH}-gfortran ${GC_SECTIONS_FLAG})
set(CMAKE_Go_COMPILER "${TOOLCHAIN_BIN_DIR}/${TARGET_ARCH}-gccgo" "${GC_SECTIONS_FLAG}")

# hinting the prefix and location is needed in order to correctly detect
# binutils
set(_CMAKE_TOOLCHAIN_PREFIX "${TARGET_ARCH}-")
set(_CMAKE_TOOLCHAIN_LOCATION ${TOOLCHAIN_BIN_DIR})

option(HAVE_ARCH_MEMSET	 "Use machine specific version of memset" ON)
option(HAVE_ARCH_MEMCPY	 "Use machine specific version of memcpy" ON)
option(HAVE_ARCH_STRLEN	 "Use machine specific version of strlen" ON)
option(HAVE_ARCH_STRCPY	 "Use machine specific version of strcpy" ON)
option(HAVE_ARCH_STRNCPY "Use machine specific version of strncpy" ON)
