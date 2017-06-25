include(${CMAKE_CURRENT_LIST_DIR}/HermitCore-Utils.cmake)
# no include guard here because we have to include this file twice to correctly
# set CMAKE_INSTALL_PREFIX

# root of HermitCore project
set(HERMIT_ROOT ${CMAKE_CURRENT_LIST_DIR}/..)

# set default install prefix if user doesn't specify one
if(${CMAKE_INSTALL_PREFIX_INITIALIZED_TO_DEFAULT})
	# See CMake docs for reference:
	# https://cmake.org/cmake/help/v3.7/variable/CMAKE_INSTALL_PREFIX_INITIALIZED_TO_DEFAULT.html
	set(CMAKE_INSTALL_PREFIX /opt/hermit CACHE PATH "..." FORCE)
endif()

# we install 3rd party libraries to an intermediate directory and relocate
# them here later when installing the whole project
if(NOT LOCAL_PREFIX_BASE_DIR)
	# will be injected into external project because CMAKE_BINARY_DIR will be
	# different there
	set(LOCAL_PREFIX_BASE_DIR ${CMAKE_BINARY_DIR}/local_prefix)
endif()

# during build process libraries and external projects will be deployed into
# this directory structure
set(LOCAL_PREFIX_DIR ${LOCAL_PREFIX_BASE_DIR}/${CMAKE_INSTALL_PREFIX})
set(LOCAL_PREFIX_ARCH_DIR ${LOCAL_PREFIX_DIR}/${TARGET_ARCH})
set(LOCAL_PREFIX_ARCH_INCLUDE_DIR ${LOCAL_PREFIX_ARCH_DIR}/include)

# when building applications within the HermitCore project (tests, ...) they
# will link prefarably against libraries in this directory in order to test
# changes in the kernel
set(LOCAL_PREFIX_ARCH_LIB_DIR ${LOCAL_PREFIX_ARCH_DIR}/lib)

# generated configs will be put here
set(GENERATED_CONFIG_DIR ${CMAKE_BINARY_DIR}/include)
