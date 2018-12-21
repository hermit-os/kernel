cmake_minimum_required(VERSION 3.7)
include(../cmake/HermitCore-Utils.cmake)

project(loader C)

# Add the Cargo project to build the Rust library.
set(LOADER_RS "${CMAKE_BINARY_DIR}/loader/${HERMIT_ARCH}-unknown-hermit-kernel/release/libhermit_loader.a")
add_custom_target(loader_rs
	COMMAND
		${CMAKE_COMMAND} -E env CARGO_TARGET_DIR=${CMAKE_BINARY_DIR}/loader RUST_TARGET_PATH=`pwd`/../target
		cargo xbuild --release --target ${HERMIT_ARCH}-unknown-hermit-kernel
	WORKING_DIRECTORY
		${CMAKE_CURRENT_LIST_DIR})

# Assemble the entry point into a library and make it depend on the Rust library.
if("${HERMIT_ARCH}" STREQUAL "aarch64")
	enable_language(ASM)
	find_toolchain_program(mkimage)
	find_toolchain_program(readelf)
	set(LOADER_ENTRY "src/arch/aarch64/entry.S")
elseif("${HERMIT_ARCH}" STREQUAL "x86_64")
	enable_language(ASM_NASM)
	set(LOADER_ENTRY "src/arch/x86_64/entry.asm")
endif()

add_library(loader_entry STATIC ${LOADER_ENTRY})
add_dependencies(loader_entry loader_rs)
set_source_files_properties("${LOADER_ENTRY}" PROPERTIES OBJECT_DEPENDS ${LOADER_RS})

# Merge the Rust library into the entry point library after assembling.
add_custom_command(
	TARGET loader_entry POST_BUILD

	COMMAND
		${CMAKE_AR} x ${LOADER_RS}
	COMMAND
		${CMAKE_AR} rcs $<TARGET_FILE:loader_entry> *.o
	COMMAND
		${CMAKE_COMMAND} -E remove *.o)

# Use the C compiler to link everything into the loader executable using the linker script.
set(CMAKE_C_LINK_EXECUTABLE "${CMAKE_LINKER} <CMAKE_C_LINK_FLAGS> <LINK_FLAGS> <OBJECTS> -o <TARGET> <LINK_LIBRARIES>")
set(CMAKE_EXE_LINKER_FLAGS "-n --gc-sections -T ${CMAKE_CURRENT_LIST_DIR}/src/arch/${HERMIT_ARCH}/link.ld")
add_executable(loader dummy.c)
target_link_libraries(loader loader_entry)
set(LOADER_ELF ldhermit.elf)
set_target_properties(loader PROPERTIES OUTPUT_NAME ${LOADER_ELF})

if("${HERMIT_ARCH}" STREQUAL "aarch64")
	string(REPLACE ".elf" ".bin" LOADER_BIN ${LOADER_ELF})
	string(REPLACE ".elf" ".uboot" LOADER_UBOOT ${LOADER_ELF})

	# Get the loading and entry poing address from the ELF file.
	# Then convert the ELF into a binary and finally into a U-Boot image for QEMU and other ARM platforms.
	add_custom_command(
		TARGET
			loader POST_BUILD
		COMMAND
			${CMAKE_READELF} -h ${LOADER_ELF} | grep "Entry point address" | gawk "{print $4}" > entry_point_address
		COMMAND
			${CMAKE_OBJCOPY} -O binary ${LOADER_ELF} ${LOADER_BIN}
		COMMAND
			bash -c "${CMAKE_MKIMAGE} -A arm64 -O linux -T kernel -C none -a `cat entry_point_address` -e `cat entry_point_address` -n hermit-loader -d ${LOADER_BIN} ${LOADER_UBOOT}"
		VERBATIM)

	install(FILES ${CMAKE_CURRENT_BINARY_DIR}/${LOADER_UBOOT} DESTINATION bin)
elseif("${HERMIT_ARCH}" STREQUAL "x86_64")
	# Make the loader executable a 32-bit ELF file for QEMU.
	add_custom_command(
		TARGET
			loader POST_BUILD
		COMMAND
			${CMAKE_OBJCOPY} -O elf32-i386 $<TARGET_FILE:loader>)
endif()

# Install the loader in the bin directory.
install(TARGETS loader DESTINATION bin)
