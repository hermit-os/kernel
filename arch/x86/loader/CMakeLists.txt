cmake_minimum_required(VERSION 3.7)

project(arch_x86_loader C ASM_NASM)

## ASM sources

file(GLOB ASM_SOURCES *.asm)
add_library(arch_x86_loader_asm	STATIC ${ASM_SOURCES})

## C sources

file(GLOB C_SOURCES *.c)
add_executable(arch_x86_loader ${C_SOURCES})

target_include_directories(arch_x86_loader
	PRIVATE include/)

target_compile_options(arch_x86_loader
	PRIVATE -O2 -Wall -m64 -std=gnu99 -ffreestanding -mno-red-zone
			-fstrength-reduce -fomit-frame-pointer -finline-functions)

target_link_libraries(arch_x86_loader
	arch_x86_loader_asm
	"-T ${CMAKE_CURRENT_LIST_DIR}/link.ld"
	"-z max-page-size=4096"
	-Wl,--build-id=none		# required because CMake links with gcc, not ld
	-nostdlib)

# tools/proxy looks for `ldhermit.elf`
set_target_properties(arch_x86_loader PROPERTIES
	OUTPUT_NAME ldhermit.elf)

add_custom_command(
	TARGET arch_x86_loader POST_BUILD
	# Split debug symbols into seperate file
	COMMAND
		${CMAKE_OBJCOPY} --only-keep-debug
							$<TARGET_FILE:arch_x86_loader>
							$<TARGET_FILE:arch_x86_loader>.sym
	# Qemu requires 32-bit ELF
	COMMAND
		${CMAKE_OBJCOPY} -O elf32-i386 --strip-debug
							$<TARGET_FILE:arch_x86_loader>)

install(TARGETS arch_x86_loader
	DESTINATION bin)

# Show include files in IDE
file(GLOB_RECURSE ARCH_X86_LOADER_INCLUDES "include/*")
add_custom_target(arch_x86_loader_includes_ide SOURCES ${ARCH_X86_LOADER_INCLUDES})
