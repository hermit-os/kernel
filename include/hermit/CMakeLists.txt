cmake_minimum_required(VERSION 3.7)

configure_file(config.h.in config.h)
configure_file(config.asm.in config.asm)

# Show include files in IDE
file(GLOB_RECURSE HERMIT_INCLUDES "*")
add_custom_target(hermit_includes_ide SOURCES ${HERMIT_INCLUDES})

# install generated config files when building libhermit for bootstrapping
install(FILES
	${GENERATED_CONFIG_DIR}/hermit/config.h
	${GENERATED_CONFIG_DIR}/hermit/config.asm
	DESTINATION ${TARGET_ARCH}/include/hermit/
	COMPONENT bootstrap)
