include(${CMAKE_CURRENT_LIST_DIR}/HermitCore-Utils.cmake)
include_guard()

include(${CMAKE_CURRENT_LIST_DIR}/HermitCore-Paths.cmake)
include(${CMAKE_CURRENT_LIST_DIR}/HermitCore-Configuration.cmake)

# scripts to detect HermitCore Go compiler
list(APPEND CMAKE_MODULE_PATH ${CMAKE_CURRENT_LIST_DIR}/golang/)

if(NOT HERMIT_ARCH)
	execute_process(COMMAND uname -m OUTPUT_VARIABLE HERMIT_ARCH OUTPUT_STRIP_TRAILING_WHITESPACE)
endif()

if(NOT CMAKE_BUILD_TYPE)
	set(CMAKE_BUILD_TYPE Release)
endif()

if(CMAKE_BUILD_TYPE MATCHES Release)
	set(CARGO_BUILDTYPE_OUTPUT "release")
	set(CARGO_BUILDTYPE_PARAMETER "--release")
else()
	set(CARGO_BUILDTYPE_OUTPUT "debug")
	set(CARGO_BUILDTYPE_PARAMETER "")
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

if(MTUNE)
	set(HERMIT_KERNEL_FLAGS ${HERMIT_KERNEL_FLAGS} -mtune=${MTUNE})
	set(HERMIT_APP_FLAGS    ${HERMIT_APP_FLAGS}    -mtune=${MTUNE})
endif()

set(HERMIT_KERNEL_INCLUDES
    ${CMAKE_BINARY_DIR}/include
    ${HERMIT_ROOT}/include
    ${HERMIT_ROOT}/include/hermit/${HERMIT_ARCH}
    ${HERMIT_ROOT}/lwip/src/include)

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
