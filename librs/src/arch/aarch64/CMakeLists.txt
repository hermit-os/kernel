enable_language(ASM)


# aarch64-specific sources (assembled with GAS)
set(ARCH_SOURCES
	"${CMAKE_CURRENT_LIST_DIR}/entry.S")
add_library(arch OBJECT ${ARCH_SOURCES})
target_compile_options(arch PRIVATE ${HERMIT_KERNEL_FLAGS})
target_include_directories(arch PRIVATE ${HERMIT_KERNEL_INCLUDES})

set(ARCH_OBJECTS $<TARGET_OBJECTS:arch> PARENT_SCOPE)

# Set a source-level dependency from the entry point on the Rust library.
# This causes the entry point to be reassembled when the Rust library changes and subsequently the Hermit library is relinked.
set_source_files_properties("${CMAKE_CURRENT_LIST_DIR}/entry.S" PROPERTIES OBJECT_DEPENDS ${HERMIT_RS})

#set_source_files_properties("${CMAKE_CURRENT_LIST_DIR}/entry.S" PROPERTIES COMPILE_FLAGS "-x assembler-with-cpp")
