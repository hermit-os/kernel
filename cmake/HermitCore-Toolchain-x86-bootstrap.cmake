include(${CMAKE_CURRENT_LIST_DIR}/HermitCore-Toolchain-x86.cmake)
include_guard()

set(CMAKE_C_COMPILER_WORKS 1 CACHE INTERNAL "")
set(CMAKE_CXX_COMPILER_WORKS 1 CACHE INTERNAL "")

# unset unneeded compilers
unset(CMAKE_Go_COMPILER)
unset(CMAKE_Fortran_COMPILER)

set(BOOTSTRAP true)
