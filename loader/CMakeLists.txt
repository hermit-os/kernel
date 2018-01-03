cmake_minimum_required(VERSION 3.7)

project(loader C ASM_NASM)

# Add the Cargo project to build the Rust library libhermit_loader.a.
add_custom_target(loader_rs
	COMMAND
		${CMAKE_COMMAND} -E env CARGO_TARGET_DIR=${CMAKE_BINARY_DIR}/loader
		cargo build --release --target x86_64-hermit
	WORKING_DIRECTORY
		${CMAKE_CURRENT_LIST_DIR})

# Assemble the entry point into a library.
add_library(loader_entry STATIC "src/entry.asm")
add_dependencies(loader_entry loader_rs)

# Merge the Rust output into the entry point library.
add_custom_command(
	TARGET loader_entry POST_BUILD

	COMMAND
		${CMAKE_AR} x ${CMAKE_BINARY_DIR}/loader/x86_64-hermit/release/libhermit_loader.a
	COMMAND
		${CMAKE_AR} rcs $<TARGET_FILE:loader_entry> *.o
	COMMAND
		${CMAKE_COMMAND} -E remove *.o)

# Use the C compiler to link the entry point into an executable using the linker script.
set(CMAKE_C_LINK_EXECUTABLE "${CMAKE_LINKER} <CMAKE_C_LINK_FLAGS> <LINK_FLAGS> <OBJECTS> -o <TARGET> <LINK_LIBRARIES>")
set(CMAKE_EXE_LINKER_FLAGS "-n --gc-sections -T ${CMAKE_CURRENT_LIST_DIR}/src/link.ld")
add_executable(loader dummy.c)
target_link_libraries(loader loader_entry)
set_target_properties(loader PROPERTIES OUTPUT_NAME ldhermit.elf)

# Post-process the loader executable.
add_custom_command(
	TARGET loader POST_BUILD

	# Make it a 32-bit ELF file for QEMU.
	COMMAND
		${CMAKE_OBJCOPY} -O elf32-i386 $<TARGET_FILE:loader>)

# Install the loader in the bin directory.
install(TARGETS loader DESTINATION bin)
