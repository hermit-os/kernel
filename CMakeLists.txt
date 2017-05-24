cmake_minimum_required(VERSION 3.7)

include(ExternalProject)
include(cmake/HermitCore.cmake)

project (HermitCore)

### Kernel

# generate config files
add_subdirectory(include/hermit)

add_kernel_module_sources("kernel"		"kernel/*.c")
add_kernel_module_sources("libkern"		"libkern/*.c")
add_kernel_module_sources("mm"			"mm/*.c")
add_kernel_module_sources("drivers"		"drivers/net/*.c")

set(LWIP_SRC lwip/src)
add_kernel_module_sources("lwip"	"${LWIP_SRC}/api/*.c")
add_kernel_module_sources("lwip"	"${LWIP_SRC}/arch/*.c")
add_kernel_module_sources("lwip"	"${LWIP_SRC}/core/*.c")
add_kernel_module_sources("lwip"	"${LWIP_SRC}/core/ipv4/*.c")
add_kernel_module_sources("lwip"	"${LWIP_SRC}/core/ipv6/*.c")
add_kernel_module_sources("lwip"	"${LWIP_SRC}/netif/*.c")

get_kernel_modules(KERNEL_MODULES)
foreach(MODULE ${KERNEL_MODULES})
	get_kernel_module_sources(SOURCES ${MODULE})

	# maintain list of all objects that will end up in libhermit.a
	list(APPEND KERNEL_OBJECTS $<TARGET_OBJECTS:${MODULE}>)

	add_library(${MODULE} OBJECT ${SOURCES})

	# this is kernel code
	target_compile_definitions(${MODULE}
		PRIVATE -D__KERNEL__)

	target_compile_options(${MODULE}
		PRIVATE ${HERMIT_KERNEL_FLAGS})

	target_include_directories(${MODULE}
		PUBLIC ${HERMIT_KERNEL_INCLUDES})

	# suppress all LwIP compiler warnings. Not our code, so we cannot fix
	if("${MODULE}" STREQUAL "lwip")
		target_compile_options(${MODULE}
			PRIVATE -w)
	endif()

endforeach()

# add arch/x86 and its objects
# TODO: make this conditional when new architectures are implemented
add_subdirectory(arch/x86)
list(APPEND KERNEL_OBJECTS
	$<TARGET_OBJECTS:${X86_KERNEL_ASM_TARGET}>
	$<TARGET_OBJECTS:${X86_KERNEL_C_TARGET}>)

# finally build libhermit.a
add_library(hermit-bootstrap STATIC ${KERNEL_OBJECTS})
set_target_properties(hermit-bootstrap PROPERTIES
	ARCHIVE_OUTPUT_NAME hermit)

# after compiling ASM sources, we need to post-process them. Adding this
# dependency makes sure that this is done before hermit is linked
add_dependencies(hermit-bootstrap ${X86_KERNEL_TARGET})

add_custom_command(
	TARGET
		hermit-bootstrap POST_BUILD
	# rename sections in final library
	COMMAND
		${CMAKE_OBJCOPY} --rename-section .bss=.kbss
						 --rename-section .text=.ktext
						 --rename-section .data=.kdata
						 $<TARGET_FILE:hermit-bootstrap>

	# copy libhermit.a into local prefix directory so that all subsequent
	# targets can link against the freshly built version (as opposed to
	# linking against the one supplied by the toolchain)
	COMMAND
		${CMAKE_COMMAND} -E make_directory ${LOCAL_PREFIX_ARCH_LIB_DIR}
	COMMAND
		${CMAKE_COMMAND} -E copy_if_different
							$<TARGET_FILE:hermit-bootstrap>
							${LOCAL_PREFIX_ARCH_LIB_DIR}/

	# and also copy headers into local prefix
	COMMAND
		${CMAKE_COMMAND} -E make_directory ${LOCAL_PREFIX_ARCH_INCLUDE_DIR}/hermit
	COMMAND
		${CMAKE_COMMAND} -E copy_if_different
							${CMAKE_BINARY_DIR}/include/hermit/*.h
							${LOCAL_PREFIX_ARCH_INCLUDE_DIR}/hermit/
	COMMAND
		${CMAKE_COMMAND} -E copy_if_different
							${CMAKE_BINARY_DIR}/include/hermit/*.asm
							${LOCAL_PREFIX_ARCH_INCLUDE_DIR}/hermit/)


# deploy libhermit.a and headers for package creation
install(TARGETS hermit-bootstrap
	DESTINATION ${TARGET_ARCH}/lib
	COMPONENT bootstrap)

install(DIRECTORY include/hermit
	DESTINATION ${TARGET_ARCH}/include/
	COMPONENT bootstrap
	FILES_MATCHING
		PATTERN *.h)

# provide custom target to only install libhermit without its runtimes which is
# needed during the compilation of the cross toolchain
add_custom_target(hermit-bootstrap-install
	DEPENDS
		hermit-bootstrap
	COMMAND
		${CMAKE_COMMAND}
			-DCMAKE_INSTALL_COMPONENT=bootstrap
			-DCMAKE_INSTALL_PREFIX=${CMAKE_INSTALL_PREFIX}
			-P cmake_install.cmake)

# The target 'hermit' includes the HermitCore kernel and several runtimes.
# Applications should depend on this target if they link against HermitCore.
add_custom_target(hermit
	DEPENDS hermit-bootstrap)


### External projects
#
# Build projects externally and deploy into temporary common prefix, will later
# be relocated for installation

## HermitCore's own tools such as Qemu/KVM proxy
build_external(tools ${HERMIT_ROOT}/tools "")
build_external(arch_x86_loader ${HERMIT_ROOT}/arch/x86/loader "")

## Intel's OpenMP runtime for x86 (libomp)
build_external(libiomp ${HERMIT_ROOT}/usr/libomp ""
		-DHERMIT=1
		-DCMAKE_TOOLCHAIN_FILE=${CMAKE_TOOLCHAIN_FILE}
		-DCMAKE_INSTALL_PREFIX=${CMAKE_INSTALL_PREFIX}/${TARGET_ARCH})

# libomp is part of HermitCore's runtime and should be available before any
# application will link
add_dependencies(hermit libiomp)

## iRCCE
build_external(ircce ${HERMIT_ROOT}/usr/ircce "")
add_dependencies(hermit ircce)

## XRay profiler
build_external(xray ${HERMIT_ROOT}/usr/xray "")
add_dependencies(hermit xray)

## Tests and benchmarks
build_external(tests ${HERMIT_ROOT}/usr/tests hermit)
build_external(benchmarks ${HERMIT_ROOT}/usr/benchmarks hermit)
build_external(openmpbench ${HERMIT_ROOT}/usr/openmpbench hermit)

## relocate the local prefix to our install destination
install(DIRECTORY ${LOCAL_PREFIX_DIR}/
	DESTINATION ${CMAKE_INSTALL_PREFIX}/
	USE_SOURCE_PERMISSIONS)


### QEmu
# Start HermitCore as multi-kernel in a QEmu VM

add_custom_target(qemu
	COMMAND
		qemu-system-x86_64
			-machine accel=kvm -cpu host
			-smp 10 -m 8G -numa node,nodeid=0,cpus=0-4 -numa node,nodeid=1,cpus=5-9
			-kernel ${HERMIT_ROOT}/config/bzImage
			-append "root=/dev/ram0 rootfstype=ramfs init=init console=ttyS0"
			-net nic,model=rtl8139 -net user -net dump
			-nographic -monitor telnet:127.0.0.1:1235,server,nowait
			-fsdev local,security_model=none,id=fsdev0,path=${LOCAL_PREFIX_DIR}
			-device virtio-9p-pci,id=fs0,fsdev=fsdev0,mount_tag=hermit
			-s
	USES_TERMINAL VERBATIM)

# create a QEmu target that depends on everything
get_property(_TARGETS
	DIRECTORY .
	PROPERTY BUILDSYSTEM_TARGETS)

add_custom_target(qemu-dep
	DEPENDS
		${_TARGETS} qemu)


### Packaging

set(CPACK_PACKAGE_NAME libhermit)
set(CPACK_SYSTEM_NAME all)

set(CPACK_PACKAGE_VERSION_MAJOR 0)
set(CPACK_PACKAGE_VERSION_MINOR 2)
set(CPACK_PACKAGE_VERSION_PATCH 0)

set(CPACK_PACKAGE_CONTACT "Stefan Lankes <slankes@eonerc.rwth-aachen.de>")

# build .deb, .rpm and .tar.bz2 packages
set(CPACK_GENERATOR DEB;RPM;TBZ2)

# needed in order for tests and bechmark to use correct install prefix
set(CPACK_SET_DESTDIR on)

## Debian specific
# not dependent on Debian system architecture
set(CPACK_DEBIAN_PACKAGE_ARCHITECTURE all)

## RPM specific
# libhermit is currently not relocatable
set(CPACK_PACKAGE_RELOCATABLE FALSE)

include(CPack)
