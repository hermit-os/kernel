cmake_minimum_required(VERSION 3.7)
include(../../cmake/HermitCore.cmake)

project(hermit_ircce C)

add_compile_options(${HERMIT_APP_FLAGS})

file(GLOB SOURCES *.c)

add_library(ircce STATIC ${SOURCES})

# deployment
install(TARGETS ircce
	DESTINATION ${TARGET_ARCH}/lib)
install(FILES
			iRCCE.h iRCCE_lib.h RCCE_debug.h RCCE.h RCCE_lib.h rte_memcpy.h
		DESTINATION
			${TARGET_ARCH}/include)
