project(librs C ASM_NASM)

if(EXISTS "${CMAKE_SOURCE_DIR}/.git")
	find_package(Git)
	if(GIT_FOUND)
		execute_process(
			COMMAND "${GIT_EXECUTABLE}" rev-parse HEAD
			WORKING_DIRECTORY ${CMAKE_SOURCE_DIR}
			OUTPUT_VARIABLE GIT_COMMIT_HASH
			RESULT_VARIABLE GIT_CALL_RESULT
			OUTPUT_STRIP_TRAILING_WHITESPACE
		)
		if(GIT_CALL_RESULT EQUAL 0)
			set(COMMIT_HASH "${GIT_COMMIT_HASH}")
		endif()
	endif()
endif()

configure_file(config.rs.in ${CMAKE_BINARY_DIR}/hermit_rs/config.rs)

if("${HERMIT_ARCH}" STREQUAL "x86_64")
	# Preprocess the PCI IDs into a Rust array.
	add_custom_command(
		OUTPUT
			${CMAKE_BINARY_DIR}/hermit_rs/pcidata.rs
		DEPENDS
			pci_ids_parser
		DEPENDS
			${CMAKE_SOURCE_DIR}/pci_ids_parser/pci.ids
		COMMAND
			${CMAKE_BINARY_DIR}/pci_ids_parser/release/pci_ids_parser
			${CMAKE_SOURCE_DIR}/pci_ids_parser/pci.ids
			${CMAKE_BINARY_DIR}/hermit_rs/pcidata.rs
		VERBATIM)
	add_custom_target(pcidata
		DEPENDS
			${CMAKE_BINARY_DIR}/hermit_rs/pcidata.rs)

	# Preprocess the SMP Boot Code into a Rust array.
	add_custom_command(
		OUTPUT
			${CMAKE_BINARY_DIR}/hermit_rs/smp_boot_code.rs
		DEPENDS
			${CMAKE_CURRENT_LIST_DIR}/src/arch/x86_64/boot.asm
		COMMAND
			nasm -f bin -o boot.bin ${CMAKE_CURRENT_LIST_DIR}/src/arch/x86_64/boot.asm
		COMMAND
			echo -n "static SMP_BOOT_CODE: [u8; " > smp_boot_code.rs
		COMMAND
			stat -c %s boot.bin >> smp_boot_code.rs
		COMMAND
			echo "] = [" >> smp_boot_code.rs
		COMMAND
			hexdump -v -e "1/1 \"0x%02X,\"" boot.bin >> smp_boot_code.rs
		COMMAND
			echo "];" >> smp_boot_code.rs
		WORKING_DIRECTORY
			${CMAKE_BINARY_DIR}/hermit_rs
		VERBATIM)
	add_custom_target(smp_boot_code
		DEPENDS
			${CMAKE_BINARY_DIR}/hermit_rs/smp_boot_code.rs)
endif()

# Add the Cargo project to build the Rust library.
set(HERMIT_RS "${CMAKE_BINARY_DIR}/hermit_rs/${TARGET_ARCH}/${CARGO_BUILDTYPE_OUTPUT}/libhermit_rs.a")
add_custom_target(hermit_rs
	DEPENDS
		objmv
	COMMAND
		${CMAKE_COMMAND} -E env CARGO_TARGET_DIR=${CMAKE_BINARY_DIR}/hermit_rs RUST_TARGET_PATH=${HERMIT_ROOT}/target
		xargo build ${CARGO_BUILDTYPE_PARAMETER} --target ${TARGET_ARCH}
	WORKING_DIRECTORY
		${CMAKE_CURRENT_LIST_DIR})

if("${HERMIT_ARCH}" STREQUAL "x86_64")
	add_dependencies(hermit_rs pcidata)
	add_dependencies(hermit_rs smp_boot_code)
endif()

# Add a documentation target for the Cargo project.
add_custom_target(doc
	COMMAND
		${CMAKE_COMMAND} -E env CARGO_TARGET_DIR=${CMAKE_BINARY_DIR}/hermit_rs
		cargo rustdoc -- --no-defaults --passes collapse-docs --passes unindent-comments
	WORKING_DIRECTORY
		${CMAKE_CURRENT_LIST_DIR})


# Now define "kernel modules", which are built into object libraries.
# It is important that ASM and C sources are not mixed in a single module and ASM modules are suffixed with "_asm"!
add_definitions(-D__KERNEL__)

# arch
if("${HERMIT_ARCH}" STREQUAL "x86_64")
	add_kernel_module_sources("arch_asm" "${CMAKE_CURRENT_LIST_DIR}/src/arch/x86_64/entry.asm")
	add_kernel_module_sources("arch_asm" "${CMAKE_CURRENT_LIST_DIR}/src/arch/x86_64/sighandler.asm")
	add_kernel_module_sources("arch_asm" "${CMAKE_CURRENT_LIST_DIR}/src/arch/x86_64/switch.asm")
endif()

# LwIP
set(LWIP_SRC ${CMAKE_SOURCE_DIR}/lwip/src)
add_kernel_module_sources("lwip" "${LWIP_SRC}/api/*.c")
add_kernel_module_sources("lwip" "${LWIP_SRC}/arch/*.c")
add_kernel_module_sources("lwip" "${LWIP_SRC}/core/*.c")
add_kernel_module_sources("lwip" "${LWIP_SRC}/core/ipv4/*.c")
add_kernel_module_sources("lwip" "${LWIP_SRC}/core/ipv6/*.c")
add_kernel_module_sources("lwip" "${LWIP_SRC}/netif/*.c")

# Support for Go applications that currently cannot be implemented in Rust
if("${HERMIT_ARCH}" STREQUAL "x86_64")
	add_kernel_module_sources("libgosupport_asm" "${CMAKE_SOURCE_DIR}/libgosupport/context.asm")
endif()

add_kernel_module_sources("libgosupport" "${CMAKE_SOURCE_DIR}/libgosupport/context.c")


# Add object libraries and set options for the defined kernel modules.
get_kernel_modules(KERNEL_MODULES)
foreach(MODULE ${KERNEL_MODULES})
	get_kernel_module_sources(SOURCES ${MODULE})

	# Maintain a list of all objects that will end up in the ASM/C library.
	list(APPEND KERNEL_OBJECTS $<TARGET_OBJECTS:${MODULE}>)

	add_library(${MODULE} OBJECT ${SOURCES})
	add_dependencies(${MODULE} hermit_rs)

	# Set C compiler options and include directories for non-ASM modules
	if(NOT "${MODULE}" MATCHES "_asm$")
		target_compile_options(${MODULE}
			PRIVATE ${HERMIT_KERNEL_FLAGS})

		target_include_directories(${MODULE}
			PUBLIC ${HERMIT_KERNEL_INCLUDES})
	endif()

	# Suppress all LwIP compiler warnings. Not our code, so we cannot fix
	if("${MODULE}" STREQUAL "lwip")
		target_compile_options(${MODULE} PRIVATE -w)
	endif()
endforeach()

# Set a source-level dependency from the entry point on the Rust library.
# This causes the entry point to be reassembled when the Rust library changes and subsequently the Hermit library is relinked.
if("${HERMIT_ARCH}" STREQUAL "x86_64")
	set_source_files_properties("${CMAKE_CURRENT_LIST_DIR}/src/arch/x86_64/entry.asm" PROPERTIES OBJECT_DEPENDS ${HERMIT_RS})
endif()

# Build all kernel modules into a single static library.
add_library(hermit-bootstrap STATIC ${KERNEL_OBJECTS})
set_target_properties(hermit-bootstrap PROPERTIES ARCHIVE_OUTPUT_NAME hermit)

# Post-process the static library.
add_custom_command(
	TARGET hermit-bootstrap POST_BUILD

	# Merge the Rust library into this static library.
	COMMAND
		${CMAKE_AR} x ${HERMIT_RS}
	COMMAND
		${CMAKE_AR} rcs $<TARGET_FILE:hermit-bootstrap> *.o
	COMMAND
		${CMAKE_COMMAND} -E remove *.o

	# Convert the combined library to HermitCore's osabi
	COMMAND
		${CMAKE_ELFEDIT} --output-osabi HermitCore $<TARGET_FILE:hermit-bootstrap>

	# Rename sections in the combined library to separate this kernel data from later user data.
	COMMAND
		${CMAKE_BINARY_DIR}/objmv/release/objmv $<TARGET_FILE:hermit-bootstrap>

	# Redefine some reserved Rust symbols to avoid collisions with similarly named symbols from Rust application code.
	# Note that some of them only appear in Debug builds of HermitCore!
	COMMAND
		${CMAKE_OBJCOPY}
		--redefine-sym rust_begin_unwind=HermitCore_Kernel_rust_begin_unwind
		--redefine-sym rust_oom=HermitCore_Kernel_rust_oom
		--redefine-sym __rust_alloc=HermitCore_Kernel___rust_alloc
		--redefine-sym __rust_alloc_zeroed=HermitCore_Kernel___rust_alloc_zeroed
		--redefine-sym __rust_dealloc=HermitCore_Kernel___rust_dealloc
		--redefine-sym __rust_realloc=HermitCore_Kernel___rust_realloc
		$<TARGET_FILE:hermit-bootstrap>

	# Copy libhermit.a into local prefix directory so that all subsequent
	# targets can link against the freshly built version (as opposed to
	# linking against the one supplied by the toolchain)
	COMMAND
		${CMAKE_COMMAND} -E make_directory ${LOCAL_PREFIX_ARCH_LIB_DIR}
	COMMAND
		${CMAKE_COMMAND} -E copy_if_different $<TARGET_FILE:hermit-bootstrap> ${LOCAL_PREFIX_ARCH_LIB_DIR}/

	# and also copy headers into local prefix
	COMMAND
		${CMAKE_COMMAND} -E make_directory ${LOCAL_PREFIX_ARCH_INCLUDE_DIR}/hermit
	COMMAND
		${CMAKE_COMMAND} -E copy_if_different ${CMAKE_SOURCE_DIR}/include/hermit/*.h ${LOCAL_PREFIX_ARCH_INCLUDE_DIR}/hermit/)

# Deploy libhermit.a and headers for package creation
install(TARGETS hermit-bootstrap
	DESTINATION ${TARGET_ARCH}/lib
	COMPONENT bootstrap)

install(DIRECTORY include/hermit
	DESTINATION ${TARGET_ARCH}/include/
	COMPONENT bootstrap
	FILES_MATCHING PATTERN *.h)

# Provide custom target to only install libhermit without its runtimes which is
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
