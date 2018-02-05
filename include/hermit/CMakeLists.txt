cmake_minimum_required(VERSION 3.7)

configure_file(config.h.in config.h)
configure_file(config.asm.in config.asm)

# Show include files in IDE
file(GLOB_RECURSE HERMIT_INCLUDES "*")
add_custom_target(hermit_includes_ide SOURCES ${HERMIT_INCLUDES})
