cmake_minimum_required(VERSION 3.7)
include(../../cmake/HermitCore-Application.cmake)

project(hermit_openmpbench C)

add_executable(syncbench syncbench.c common.c)
target_link_libraries(syncbench PUBLIC -liomp -pthread)

add_executable(taskbench taskbench.c common.c)
target_link_libraries(taskbench PUBLIC -liomp -pthread)

add_library(common_sched STATIC common.c)
target_compile_definitions(common_sched PUBLIC -DSCHEDBENCH)
target_link_libraries(common_sched PUBLIC -liomp -pthread)

add_executable(schedbench schedbench.c)
target_link_libraries(schedbench common_sched)

# deployment: exclude common_sched
install_local_targets(extra/benchmarks common_sched)
