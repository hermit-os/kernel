cmake_minimum_required(VERSION 3.7)
include(../../cmake/HermitCore.cmake)

project(arch_x86_kernel C ASM_NASM)

set_parent(X86_KERNEL_TARGET		${PROJECT_NAME})
set_parent(X86_KERNEL_ASM_TARGET	${X86_KERNEL_TARGET}_asm)
set_parent(X86_KERNEL_C_TARGET		${X86_KERNEL_TARGET}_c)

add_custom_target(${X86_KERNEL_TARGET})

# compiling kernel code here
add_definitions(-D__KERNEL__)


### ASM sources ###

add_library(${X86_KERNEL_ASM_TARGET} OBJECT
	kernel/entry.asm
	libkern/string.asm)

# HACK: We need to post-process the objects by running elfedit on them, but
#       there is currently no way to get the list of objects out of CMake
#       except for $<TARGET_OBJECTS:tgt>, which only works with add_library()
#       and add_executable().
#       So predict path to objects and add custom commands that depend on
#       the asm target.
#
#       Upstream issue: https://gitlab.kitware.com/cmake/cmake/issues/15226
#
set(_BUILD_DIR "${CMAKE_CURRENT_BINARY_DIR}/${CMAKE_FILES_DIRECTORY}")
set(_BUILD_DIR "${_BUILD_DIR}/${X86_KERNEL_ASM_TARGET}.dir")

get_target_property(ASM_SOURCES ${X86_KERNEL_ASM_TARGET} SOURCES)
foreach(SOURCE ${ASM_SOURCES})
	set(OBJECT "${SOURCE}.obj")
	set(OBJECT_PATH "${_BUILD_DIR}/${OBJECT}")

	# slash (/) not allowed in target names
	string(REPLACE "/" "-"
		OBJECT_TARGET_NAME
		"${OBJECT}")

	add_custom_target("${OBJECT_TARGET_NAME}"
		COMMAND
			${CMAKE_ELFEDIT} --output-osabi HermitCore ${OBJECT_PATH}
		DEPENDS
			${X86_KERNEL_ASM_TARGET})

	# make main target depend on this
	add_dependencies(${PROJECT_NAME} ${OBJECT_TARGET_NAME})
endforeach()


### C sources ###

file(GLOB KERNEL_SOURCES	"kernel/*.c")
file(GLOB MM_SOURCES		"mm/*.c")

# add boot.h as source to mark dependency boot.asm -> boot.h -> apic.c
add_library(${X86_KERNEL_C_TARGET} OBJECT
	${KERNEL_SOURCES} ${MM_SOURCES}
	${GENERATED_CONFIG_DIR}/hermit/boot.h)

target_include_directories(${X86_KERNEL_C_TARGET} BEFORE
	PUBLIC ${HERMIT_KERNEL_INCLUDES}
	PRIVATE ${GENERATED_CONFIG_DIR})

target_compile_options(${X86_KERNEL_C_TARGET}
	PRIVATE ${HERMIT_KERNEL_FLAGS})

# assemble boot.asm and dump to C-array in boot.h
add_custom_command(
	OUTPUT
		${GENERATED_CONFIG_DIR}/hermit/boot.h
	DEPENDS
		kernel/boot.asm
	COMMAND
		echo "static const uint8_t boot_code[] = {" > boot.h
	COMMAND
		nasm -f bin -o boot.bin ${CMAKE_CURRENT_LIST_DIR}/kernel/boot.asm
	COMMAND
		hexdump -v -e "7/1 \"0x%02X, \" 1/1 \" 0x%02X,\\n\"" boot.bin >> boot.h
	COMMAND
		echo "};" >> boot.h
	WORKING_DIRECTORY
		${GENERATED_CONFIG_DIR}/hermit/
	VERBATIM USES_TERMINAL)
