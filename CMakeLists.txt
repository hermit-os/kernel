project(librs C)

if(EXISTS "${CMAKE_SOURCE_DIR}/.git")
	find_package(Git)
	if(GIT_FOUND)
		execute_process(
			COMMAND "${GIT_EXECUTABLE}" rev-parse HEAD
			WORKING_DIRECTORY ${CMAKE_SOURCE_DIR}
			OUTPUT_VARIABLE GIT_COMMIT_HASH
			RESULT_VARIABLE GIT_CALL_RESULT
			OUTPUT_STRIP_TRAILING_WHITESPACE
		)
		if(GIT_CALL_RESULT EQUAL 0)
			set(COMMIT_HASH "${GIT_COMMIT_HASH}")
		endif()
	endif()
endif()

configure_file(config.rs.in ${CMAKE_BINARY_DIR}/hermit_rs/config.rs)

# Add the Cargo project to build the Rust library.
set(HERMIT_RS "${CMAKE_BINARY_DIR}/hermit_rs/${HERMIT_ARCH}-unknown-hermit-kernel/${CARGO_BUILDTYPE_OUTPUT}/libhermit_rs.a")
add_custom_target(hermit_rs
	COMMAND
		${CMAKE_COMMAND} -E env CARGO_TARGET_DIR=${CMAKE_BINARY_DIR}/hermit_rs RUST_TARGET_PATH=${HERMIT_ROOT}/target
		cargo xbuild ${CARGO_BUILDTYPE_PARAMETER} --target ${HERMIT_ARCH}-unknown-hermit-kernel
	WORKING_DIRECTORY
		${CMAKE_CURRENT_LIST_DIR})

# Add a documentation target for the Cargo project.
add_custom_target(doc
	COMMAND
		${CMAKE_COMMAND} -E env CARGO_TARGET_DIR=${CMAKE_BINARY_DIR}/hermit_rs
		cargo rustdoc -- --no-defaults --passes collapse-docs --passes unindent-comments
	WORKING_DIRECTORY
		${CMAKE_CURRENT_LIST_DIR})

# arch
if("${HERMIT_ARCH}" STREQUAL "aarch64")
	add_subdirectory(src/arch/aarch64)
elseif("${HERMIT_ARCH}" STREQUAL "x86_64")
	add_subdirectory(src/arch/x86_64/kernel)
endif()

# Build all kernel modules into a single static library.
add_library(hermit-bootstrap STATIC ${KERNEL_OBJECTS})
set_target_properties(hermit-bootstrap PROPERTIES LINKER_LANGUAGE C)
add_dependencies(hermit-bootstrap hermit_rs)
set_target_properties(hermit-bootstrap PROPERTIES ARCHIVE_OUTPUT_NAME hermit)

# Post-process the static library.
add_custom_command(
	TARGET hermit-bootstrap POST_BUILD

	# Merge the Rust library into this static library.
	COMMAND
		${CMAKE_AR} x ${HERMIT_RS}
	COMMAND
		${CMAKE_AR} rcs $<TARGET_FILE:hermit-bootstrap> *.o
	COMMAND
		${CMAKE_COMMAND} -E remove *.o

	# Convert the combined library to osabi "Standalone"
	COMMAND
		${CMAKE_ELFEDIT} --output-osabi Standalone $<TARGET_FILE:hermit-bootstrap>

	# Rename sections in the combined library to separate this kernel data from later user data.
	COMMAND
		objmv $<TARGET_FILE:hermit-bootstrap>

	# Redefine some reserved Rust symbols to avoid collisions with similarly named symbols from Rust application code.
	# Note that some of them only appear in Debug builds of HermitCore!
	COMMAND
		${CMAKE_OBJCOPY}
		--redefine-sym rust_begin_unwind=HermitCore_Kernel_rust_begin_unwind
		--redefine-sym rust_oom=HermitCore_Kernel_rust_oom
		--redefine-sym __rust_alloc=HermitCore_Kernel___rust_alloc
		--redefine-sym __rust_alloc_zeroed=HermitCore_Kernel___rust_alloc_zeroed
		--redefine-sym __rust_dealloc=HermitCore_Kernel___rust_dealloc
		--redefine-sym __rust_realloc=HermitCore_Kernel___rust_realloc
		$<TARGET_FILE:hermit-bootstrap>

	# Copy libhermit.a into local prefix directory so that all subsequent
	# targets can link against the freshly built version (as opposed to
	# linking against the one supplied by the toolchain)
	COMMAND
		${CMAKE_COMMAND} -E make_directory ${LOCAL_PREFIX_ARCH_LIB_DIR}
	COMMAND
		${CMAKE_COMMAND} -E copy_if_different $<TARGET_FILE:hermit-bootstrap> ${LOCAL_PREFIX_ARCH_LIB_DIR}/

	# and also copy headers into local prefix
	COMMAND
		${CMAKE_COMMAND} -E make_directory ${LOCAL_PREFIX_ARCH_INCLUDE_DIR}/hermit
	COMMAND
		${CMAKE_COMMAND} -E copy_if_different ${CMAKE_SOURCE_DIR}/include/hermit/*.h ${LOCAL_PREFIX_ARCH_INCLUDE_DIR}/hermit/)

# Deploy libhermit.a and headers for package creation
install(TARGETS hermit-bootstrap
	DESTINATION ${HERMIT_ARCH}-hermit/lib
	COMPONENT bootstrap)

install(DIRECTORY include/hermit
	DESTINATION ${HERMIT_ARCH}-hermit/include/
	COMPONENT bootstrap
	FILES_MATCHING PATTERN *.h)

# Provide custom target to only install libhermit without its runtimes which is
# needed during the compilation of the cross toolchain
add_custom_target(hermit-bootstrap-install
	DEPENDS
		hermit-bootstrap
	COMMAND
		${CMAKE_COMMAND}
			-DCMAKE_INSTALL_COMPONENT=bootstrap
			-DCMAKE_INSTALL_PREFIX=${CMAKE_INSTALL_PREFIX}
			-P cmake_install.cmake)

# The target 'hermit' includes the HermitCore kernel and several runtimes.
# Applications should depend on this target if they link against HermitCore.
add_custom_target(hermit
	DEPENDS hermit-bootstrap)
