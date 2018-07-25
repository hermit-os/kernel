cmake_minimum_required(VERSION 3.7)

include(ExternalProject)
include(cmake/HermitCore.cmake)

project (HermitCore)

### Kernel

# generate config files
add_subdirectory(include/hermit)

# Build the HermitCore Rust Kernel and its dependencies.
add_subdirectory(objmv)
add_subdirectory(pci_ids_parser)
add_subdirectory(librs)


if(NOT BOOTSTRAP)
	### External projects
	#
	# Build projects externally and deploy into temporary common prefix, will later
	# be relocated for installation

	## HermitCore's own tools such as Qemu/KVM proxy
	build_external(caves ${HERMIT_ROOT}/caves "")

	if("${HERMIT_ARCH}" STREQUAL "x86_64")
		build_external(loader ${HERMIT_ROOT}/loader "")
	endif()

	if(PROFILING)
		## XRay profiler
		build_external(xray ${HERMIT_ROOT}/usr/xray "")
		add_dependencies(hermit xray)
	endif()

	## Tests and benchmarks
	build_external(tests ${HERMIT_ROOT}/usr/tests hermit)
	build_external(benchmarks ${HERMIT_ROOT}/usr/benchmarks hermit)
	build_external(openmpbench ${HERMIT_ROOT}/usr/openmpbench hermit)
endif()

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

set(CPACK_PACKAGE_NAME libhermit-rs)
set(CPACK_SYSTEM_NAME all)

set(CPACK_PACKAGE_VERSION_MAJOR 0)
set(CPACK_PACKAGE_VERSION_MINOR 3)
set(CPACK_PACKAGE_VERSION_PATCH 2)

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
