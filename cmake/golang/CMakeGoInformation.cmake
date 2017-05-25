# Distributed under the OSI-approved BSD 3-Clause License.  See accompanying
# file Copyright.txt or https://cmake.org/licensing for details.

# This should be included before the _INIT variables are
# used to initialize the cache.  Since the rule variables
# have if blocks on them, users can still define them here.
# But, it should still be after the platform file so changes can
# be made to those values.

if(CMAKE_USER_MAKE_RULES_OVERRIDE)
  # Save the full path of the file so try_compile can use it.
  include(${CMAKE_USER_MAKE_RULES_OVERRIDE} RESULT_VARIABLE _override)
  set(CMAKE_USER_MAKE_RULES_OVERRIDE "${_override}")
endif()

if(CMAKE_USER_MAKE_RULES_OVERRIDE_Go)
  # Save the full path of the file so try_compile can use it.
   include(${CMAKE_USER_MAKE_RULES_OVERRIDE_Go} RESULT_VARIABLE _override)
   set(CMAKE_USER_MAKE_RULES_OVERRIDE_Go "${_override}")
endif()

# refer: /usr/share/cmake-3.7/Modules/CMakeCInformation.cmake

if(NOT CMAKE_Go_COMPILE_OBJECT)
	set(CMAKE_Go_COMPILE_OBJECT "<CMAKE_Go_COMPILER> <FLAGS> -o <OBJECT> -c <SOURCE> ")
endif()

if(NOT CMAKE_Go_LINK_EXECUTABLE)
	set(CMAKE_Go_LINK_EXECUTABLE "<CMAKE_Go_COMPILER> -pthread <LINK_FLAGS> <OBJECTS> -o <TARGET> <LINK_LIBRARIES>")
endif()
