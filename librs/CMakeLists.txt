project(librs C ASM_NASM)

# Create a directory for our Rust output.
add_custom_target(hermit_rs_dir
	COMMAND
		${CMAKE_COMMAND} -E make_directory ${CMAKE_BINARY_DIR}/hermit_rs)

# Preprocess the PCI IDs into a Rust array.
add_custom_command(
	OUTPUT
		${CMAKE_BINARY_DIR}/hermit_rs/pcidata.rs
	DEPENDS
		hermit_rs_dir
	DEPENDS
		pci_ids_parser
	DEPENDS
		${CMAKE_SOURCE_DIR}/pci_ids_parser/pci.ids
	COMMAND
		${CMAKE_BINARY_DIR}/pci_ids_parser/release/pci_ids_parser
		${CMAKE_SOURCE_DIR}/pci_ids_parser/pci.ids
		${CMAKE_BINARY_DIR}/hermit_rs/pcidata.rs
	VERBATIM)

# Preprocess the SMP Boot Code into a Rust array.
add_custom_command(
	OUTPUT
		${CMAKE_BINARY_DIR}/hermit_rs/smp_boot_code.rs
	DEPENDS
		hermit_rs_dir
	DEPENDS
		${CMAKE_CURRENT_LIST_DIR}/src/arch/x86_64/boot.asm
	COMMAND
		nasm -f bin -o boot.bin ${CMAKE_CURRENT_LIST_DIR}/src/arch/x86_64/boot.asm
	COMMAND
		echo -n "pub static SMP_BOOT_CODE: [u8; " > smp_boot_code.rs
	COMMAND
		stat -c %s boot.bin >> smp_boot_code.rs
	COMMAND
		echo "] = [" >> smp_boot_code.rs
	COMMAND
		hexdump -v -e "7/1 \"0x%02X, \" 1/1 \" 0x%02X,\\n\"" boot.bin >> smp_boot_code.rs
	COMMAND
		echo "];" >> smp_boot_code.rs
	WORKING_DIRECTORY
		${CMAKE_BINARY_DIR}/hermit_rs
	VERBATIM)

# Add the Cargo project to build the Rust library.
set(HERMIT_RS "${CMAKE_BINARY_DIR}/hermit_rs/x86_64-hermit/release/libhermit_rs.a")
add_custom_target(hermit_rs
	DEPENDS
		objmv
	DEPENDS
		${CMAKE_BINARY_DIR}/hermit_rs/pcidata.rs
	DEPENDS
		${CMAKE_BINARY_DIR}/hermit_rs/smp_boot_code.rs
	COMMAND
		${CMAKE_COMMAND} -E env CARGO_TARGET_DIR=${CMAKE_BINARY_DIR}/hermit_rs
		cargo build --release --target x86_64-hermit
	WORKING_DIRECTORY
		${CMAKE_CURRENT_LIST_DIR})

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
add_kernel_module_sources("arch_asm" "${CMAKE_CURRENT_LIST_DIR}/src/arch/x86_64/entry.asm")
add_kernel_module_sources("arch_asm" "${CMAKE_CURRENT_LIST_DIR}/src/arch/x86_64/sighandler.asm")
add_kernel_module_sources("arch_asm" "${CMAKE_CURRENT_LIST_DIR}/src/arch/x86_64/switch.asm")

# LwIP
set(LWIP_SRC ${CMAKE_SOURCE_DIR}/lwip/src)
add_kernel_module_sources("lwip" "${LWIP_SRC}/api/*.c")
add_kernel_module_sources("lwip" "${LWIP_SRC}/arch/*.c")
add_kernel_module_sources("lwip" "${LWIP_SRC}/core/*.c")
add_kernel_module_sources("lwip" "${LWIP_SRC}/core/ipv4/*.c")
add_kernel_module_sources("lwip" "${LWIP_SRC}/core/ipv6/*.c")
add_kernel_module_sources("lwip" "${LWIP_SRC}/netif/*.c")

# libkern needed for LwIP
add_kernel_module_sources("libkern" "${CMAKE_SOURCE_DIR}/libkern/*.c")

# Support for Go applications that currently cannot be implemented in Rust
add_kernel_module_sources("libgosupport_asm" "${CMAKE_SOURCE_DIR}/libgosupport/context.asm")
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
set_source_files_properties("${CMAKE_CURRENT_LIST_DIR}/src/arch/x86_64/entry.asm" PROPERTIES OBJECT_DEPENDS ${HERMIT_RS})

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

	# Redefine _Unwind_Resume to avoid collision with libgcc.a
	COMMAND
		${CMAKE_OBJCOPY} --redefine-sym _Unwind_Resume=_Unwind_Resume_rs $<TARGET_FILE:hermit-bootstrap>

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
		${CMAKE_COMMAND} -E copy_if_different ${CMAKE_BINARY_DIR}/include/hermit/*.h ${LOCAL_PREFIX_ARCH_INCLUDE_DIR}/hermit/)

# Deploy libhermit.a and headers for package creation
install(TARGETS hermit-bootstrap
	DESTINATION ${TARGET_ARCH}/lib
	COMPONENT bootstrap)

install(DIRECTORY include/hermit
	DESTINATION ${TARGET_ARCH}/include/
	COMPONENT bootstrap
	FILES_MATCHING
		PATTERN *.h)

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
