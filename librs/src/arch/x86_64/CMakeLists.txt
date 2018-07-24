# NASM detection will change binary format depending on host system, but
# we only want to generate elf64 for HermitCore
# Note: Has to be set *before* ASM_NASM is enabled
set(CMAKE_ASM_NASM_OBJECT_FORMAT elf64)

enable_language(ASM_NASM)

# NASM hack, because it requires include paths to have a trailing /, whereas
# CMake explicitly will remove it when adding includes the usual way
# Note: Has to be set *after* ASM_NASM is enabled
set(CMAKE_ASM_NASM_FLAGS
	"${CMAKE_ASM_NASM_FLAGS} -I ${CMAKE_BINARY_DIR}/include/")


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
		${CMAKE_CURRENT_LIST_DIR}/boot.asm
	COMMAND
		nasm -f bin -o boot.bin ${CMAKE_CURRENT_LIST_DIR}/boot.asm
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

add_dependencies(hermit_rs pcidata)
add_dependencies(hermit_rs smp_boot_code)

# x86_64-specific ASM sources (assembled with NASM)
set(ARCH_ASM_SOURCES
	"${CMAKE_CURRENT_LIST_DIR}/entry.asm"
	"${CMAKE_CURRENT_LIST_DIR}/sighandler.asm"
	"${CMAKE_CURRENT_LIST_DIR}/switch.asm"
	"${CMAKE_SOURCE_DIR}/libgosupport/context.asm")
add_library(arch_asm OBJECT ${ARCH_ASM_SOURCES})

# x86_64-specific C sources
set(ARCH_C_SOURCES
	"${CMAKE_SOURCE_DIR}/libgosupport/context.c")
add_library(arch_c OBJECT ${ARCH_C_SOURCES})
target_compile_options(arch_c PRIVATE ${HERMIT_KERNEL_FLAGS})
target_include_directories(arch_c PRIVATE ${HERMIT_KERNEL_INCLUDES})

set(ARCH_OBJECTS $<TARGET_OBJECTS:arch_asm> $<TARGET_OBJECTS:arch_c> PARENT_SCOPE)

# Set a source-level dependency from the entry point on the Rust library.
# This causes the entry point to be reassembled when the Rust library changes and subsequently the Hermit library is relinked.
set_source_files_properties("${CMAKE_CURRENT_LIST_DIR}/entry.asm" PROPERTIES OBJECT_DEPENDS ${HERMIT_RS})
