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
			-fomit-frame-pointer -fno-builtin -nostdlib -nostdinc -mno-sse -mno-avx -mno-mmx -mno-3dnow)

include(CheckCCompilerFlag)
check_c_compiler_flag(-fstrength-reduce HAS_STRENGTH_REDUCE)
if (HAS_STRENGTH_REDUCE)
    set(CMAKE_C_FLAGS "${CMAKE_C_FLAGS} -fstrength-reduce")
endif()

check_c_compiler_flag(-finline-functions HAS_INLINE_FUNCTIONS)
if (HAS_INLINE_FUNCTIONS)
    set(CMAKE_C_FLAGS "${CMAKE_C_FLAGS} -finline-functions")
endif()

set(CMAKE_LINKER_NAME ld CACHE STRING "Name of the binutils linker")
mark_as_advanced(CMAKE_LINKER_NAME)

find_program(CMAKE_LINKER ${CMAKE_LINKER_NAME})
mark_as_advanced(CMAKE_LINKER)

if(NOT CMAKE_LINKER)
	message(FATAL_ERROR "Could not find the GNU LD linker: ${CMAKE_LINKER_NAME}")
endif()

set(CMAKE_C_LINK_EXECUTABLE "${CMAKE_LINKER} <CMAKE_C_LINK_FLAGS> <LINK_FLAGS> <OBJECTS> -o <TARGET> <LINK_LIBRARIES>")
set(CMAKE_EXE_LINKER_FLAGS "${CMAKE_EXE_LINKER_FLAGS} -T ${CMAKE_CURRENT_LIST_DIR}/link.ld -z max-page-size=4096 --build-id=none -nostdlib -static")

target_link_libraries(arch_x86_loader arch_x86_loader_asm)

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
