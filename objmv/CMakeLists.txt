# add external project objmv
add_custom_target(objmv
	COMMAND
		${CMAKE_COMMAND} -E make_directory ${CMAKE_BINARY_DIR}/objmv
	COMMAND
		${CMAKE_COMMAND} -E env CARGO_TARGET_DIR=${CMAKE_BINARY_DIR}/objmv
		cargo build --release
	WORKING_DIRECTORY
		"${CMAKE_SOURCE_DIR}/objmv")
