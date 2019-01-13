cmake_minimum_required(VERSION 3.7)
include(../cmake/HermitCore-Utils.cmake)

project(loader C)

# Add the Cargo project to build the Rust library.
set(LOADER_PATH "${CMAKE_BINARY_DIR}/loader/${HERMIT_ARCH}-unknown-hermit-loader/release/hermit-loader")

add_custom_target(
        loader ALL
        DEPENDS ${LOADER_PATH})

add_custom_command(
	COMMAND
		${CMAKE_COMMAND} -E env CARGO_TARGET_DIR=${CMAKE_BINARY_DIR}/loader RUST_TARGET_PATH=${CMAKE_CURRENT_LIST_DIR}/../target
		cargo xbuild --release --target ${HERMIT_ARCH}-unknown-hermit-loader
	OUTPUT
		${LOADER_PATH}
	WORKING_DIRECTORY
		${CMAKE_CURRENT_LIST_DIR}
	VERBATIM)

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
			${CMAKE_OBJCOPY} -O elf32-i386 ${LOADER_PATH})
endif()

# Install the loader in the bin directory.
install(FILES ${LOADER_PATH} DESTINATION bin)
