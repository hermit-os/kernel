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
		${CMAKE_SOURCE_DIR}/target/pci.ids
	COMMAND
		pci_ids_parser
		${CMAKE_SOURCE_DIR}/target/pci.ids
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
		echo -e "//! Compiled `boot.asm`\n" > smp_boot_code.rs
	COMMAND
		echo -n "static SMP_BOOT_CODE: [u8; " >> smp_boot_code.rs
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
