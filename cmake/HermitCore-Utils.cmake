macro(include_guard)
	if(DEFINED "_INCLUDE_GUARD_${CMAKE_CURRENT_LIST_FILE}")
		return()
	endif()
	set("_INCLUDE_GUARD_${CMAKE_CURRENT_LIST_FILE}" INCLUDED)
endmacro(include_guard)

macro(add_kernel_module_sources MODULE SOURCE_GLOB)
	file(GLOB SOURCES "${SOURCE_GLOB}")

    if("${SOURCES}" STREQUAL "")
        message(FATAL_ERROR "Module '${MODULE}' has no sources")
    endif()

    # make sure modules are unique, this is needed of multiple sources
    # are added to the same module
    list(APPEND _KERNEL_MODULES "${MODULE}")
    list(REMOVE_DUPLICATES _KERNEL_MODULES)

    # append sources for module
    list(APPEND "_KERNEL_SOURCES_${MODULE}" "${SOURCES}")
endmacro(add_kernel_module_sources)


macro(get_kernel_module_sources VAR MODULE)
    set(${VAR} ${_KERNEL_SOURCES_${MODULE}})
endmacro(get_kernel_module_sources)


macro(get_kernel_modules VAR)
	set(${VAR} ${_KERNEL_MODULES})
endmacro(get_kernel_modules)


# find program in /toolchain/dir/prefix-NAME, only supply NAME
function(find_toolchain_program NAME)

    string(TOUPPER "${NAME}" NAME_UPPER)
    string(TOLOWER "${NAME}" NAME_LOWER)

    set(VARNAME "CMAKE_${NAME_UPPER}")

    find_program(${VARNAME}
        NAMES ${_CMAKE_TOOLCHAIN_PREFIX}${NAME_LOWER}
        HINTS ${TOOLCHAIN_BIN_DIR})

    if(NOT ${VARNAME})
        message(FATAL_ERROR
				"Cannot find ${_CMAKE_TOOLCHAIN_PREFIX}${NAME_LOWER}")
    endif()
endfunction(find_toolchain_program)


macro(set_parent VAR VALUE)
	set(${VAR} ${VALUE} PARENT_SCOPE)
	set(${VAR} ${VALUE})
endmacro(set_parent)

function(get_cmd_variables VAR)
	set(_OUTPUT "")

	get_cmake_property(vs VARIABLES)

	foreach(v ${vs})
		get_property(_HELPSTRING
			CACHE ${v}
			PROPERTY HELPSTRING)
		if("${_HELPSTRING}" STREQUAL "No help, variable specified on the command line.")
			list(APPEND _OUTPUT "${v}")
		endif()
	endforeach()

	set(${VAR} ${_OUTPUT} PARENT_SCOPE)
endfunction(get_cmd_variables)

# any additional parameters will be handed over to the cmake command that the
# external project is invoked with
function(build_external NAME PATH DEPENDS)
	if("${NAME}" IN_LIST PROFILE_APPS)
		set(DO_PROFILING "-DPROFILING=true")
	endif()

	# pass through all command line variables
	get_cmd_variables(CMD_VAR_NAMES)
	foreach(var ${CMD_VAR_NAMES})
		set(CMD_VARS ${CMD_VARS} -D${var}=${${var}})
	endforeach()

	ExternalProject_Add(${NAME}
		SOURCE_DIR ${PATH}
		BUILD_ALWAYS 1
		DEPENDS ${DEPENDS}
		INSTALL_COMMAND
			${CMAKE_COMMAND} --build <BINARY_DIR>
			                 --target install --
			                   DESTDIR=${LOCAL_PREFIX_BASE_DIR}
		CMAKE_ARGS
			-DCMAKE_INSTALL_PREFIX=${CMAKE_INSTALL_PREFIX}
			-DLOCAL_PREFIX_BASE_DIR=${LOCAL_PREFIX_BASE_DIR}
			-DCMAKE_INSTALL_MESSAGE=NEVER
			-DCMAKE_EXPORT_COMPILE_COMMANDS=true
			-DMAX_ARGC_ENVC=${MAX_ARGC_ENVC}
			--no-warn-unused-cli
			${DO_PROFILING}
			${CMD_VARS}
			${ARGN})

	ExternalProject_Add_Step(${NAME} relink
		COMMAND find . -maxdepth 1 -type f -executable -exec rm {} "\\\;"
		DEPENDEES configure
		DEPENDERS build
		WORKING_DIRECTORY <BINARY_DIR>)

	ExternalProject_Add_StepDependencies(${NAME} relink ${DEPENDS})
endfunction(build_external)


# additional arguments are be treated as targets that will be excluded
function(install_local_targets PATH)
	get_property(_TARGETS
		DIRECTORY .
		PROPERTY BUILDSYSTEM_TARGETS)

	if(NOT "${ARGN}" STREQUAL "")
		list(REMOVE_ITEM _TARGETS ${ARGN})
	endif()

	install(TARGETS ${_TARGETS}
		DESTINATION ${TARGET_ARCH}/${PATH})

	# if there are any .map files for profiling, install them too
	foreach(TARGET ${_TARGETS})
		install(FILES $<TARGET_FILE:${TARGET}>.map
			DESTINATION ${TARGET_ARCH}/${PATH}
			OPTIONAL)
	endforeach()
endfunction(install_local_targets)

# set variable if not yet set
macro(set_default VARNAME)
	if(NOT ${VARNAME})
		set(${VARNAME} ${ARGN})
	endif()
endmacro(set_default)
