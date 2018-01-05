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

# Post-process the compiled Rust library.
add_custom_command(
	TARGET
		hermit_rs POST_BUILD

	# Rename sections in Rust library
	COMMAND
		${CMAKE_BINARY_DIR}/objmv/release/objmv ${HERMIT_RS}

	# Convert Rust library to HermitCore's osabi
	COMMAND
		${CMAKE_ELFEDIT} --output-osabi HermitCore ${HERMIT_RS})

# The entry point
add_library(hermit_entry OBJECT "src/arch/x86_64/entry.asm")
#set_source_files_properties("src/arch/x86_64/entry.asm" PROPERTIES OBJECT_DEPENDS ${HERMIT_RS})
add_custom_target(hermit_entry_edited
	DEPENDS
		hermit_entry
	COMMAND
		${CMAKE_ELFEDIT} --output-osabi HermitCore
		"${CMAKE_CURRENT_BINARY_DIR}/${CMAKE_FILES_DIRECTORY}/hermit_entry.dir/src/arch/x86_64/entry.asm.obj")
list(APPEND KERNEL_OBJECTS $<TARGET_OBJECTS:hermit_entry>)

# LwIP
add_definitions(-D__KERNEL__)
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
add_kernel_module_sources("libgosupport" "${CMAKE_SOURCE_DIR}/libgosupport/*.c")

# Set options for the C sources.
get_kernel_modules(KERNEL_MODULES)
foreach(MODULE ${KERNEL_MODULES})
	get_kernel_module_sources(SOURCES ${MODULE})

	# Maintain a list of all objects that will end up in libhermit.a
	list(APPEND KERNEL_OBJECTS $<TARGET_OBJECTS:${MODULE}>)

	add_library(${MODULE} OBJECT ${SOURCES})

	target_compile_options(${MODULE}
		PRIVATE ${HERMIT_KERNEL_FLAGS})

	target_include_directories(${MODULE}
		PUBLIC ${HERMIT_KERNEL_INCLUDES})

	# Suppress all LwIP compiler warnings. Not our code, so we cannot fix
	if("${MODULE}" STREQUAL "lwip")
		target_compile_options(${MODULE}
			PRIVATE -w)
	endif()
endforeach()

# Build the C library libhermit.a out of the entry point and kernel modules.
add_library(hermit-bootstrap STATIC ${KERNEL_OBJECTS})
set_target_properties(hermit-bootstrap PROPERTIES
	ARCHIVE_OUTPUT_NAME hermit)
add_dependencies(hermit-bootstrap
	hermit_entry_edited
	hermit_rs)

# Post-process the compiled C library.
add_custom_command(
	TARGET
		hermit-bootstrap POST_BUILD

	# Rename sections in final library
	COMMAND
		${CMAKE_OBJCOPY} --rename-section .bss=.kbss
						 --rename-section .text=.ktext
						 --rename-section .data=.kdata
						 $<TARGET_FILE:hermit-bootstrap>

	# Merge the Rust library into the C library.
	COMMAND
		${CMAKE_AR} x ${HERMIT_RS}
	COMMAND
		${CMAKE_AR} rcs $<TARGET_FILE:hermit-bootstrap> *.o
	COMMAND
		${CMAKE_COMMAND} -E remove *.o

	# Redefine _Unwind_Resume to avoid collision with libgcc.a
	COMMAND
		${CMAKE_OBJCOPY} --redefine-sym _Unwind_Resume=_Unwind_Resume_rs $<TARGET_FILE:hermit-bootstrap>)

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
