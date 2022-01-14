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

add_dependencies(hermit_rs pcidata)
