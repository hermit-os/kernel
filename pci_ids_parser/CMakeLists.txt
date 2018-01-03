# add external project pci_ids_parser
add_custom_target(pci_ids_parser
	#COMMAND
	#	${CMAKE_COMMAND} -E make_directory ${CMAKE_BINARY_DIR}/pci_ids_parser
	COMMAND
		${CMAKE_COMMAND} -E env CARGO_TARGET_DIR=${CMAKE_BINARY_DIR}/pci_ids_parser
		cargo build --release
	WORKING_DIRECTORY
		"${CMAKE_SOURCE_DIR}/pci_ids_parser")
