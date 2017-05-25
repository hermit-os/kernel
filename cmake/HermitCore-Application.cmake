include(${CMAKE_CURRENT_LIST_DIR}/HermitCore.cmake)
include_guard()

add_compile_options(${HERMIT_APP_FLAGS})

# link against and include locally built libraries instead of the ones
# supplied with the toolchain, if built from top-level
link_directories(${LOCAL_PREFIX_ARCH_LIB_DIR})
include_directories(BEFORE ${LOCAL_PREFIX_ARCH_INCLUDE_DIR})
