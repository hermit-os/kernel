HermitCore CMake build system
=============================

HermitCore requires at least CMake version `3.7`, which (at the time of
introduction) is not yet available on most Linux distributions. We therefore
provide a script `cmake/local-cmake.sh` that fetches precompiled binaries from
the CMake project and installs them locally in `cmake/cmake-3.7*`. Only when
sourced for the first time it will download CMake, on further runs it detects
the existing download and adds it to `PATH` automatically.

```bash
$ . cmake/local-cmake.sh
-- Downloading CMake
cmake-3.7.2-Linux-x 100%[=================>]  29,26M   837KB/s    in 19s
-- Unpacking CMake
-- Local CMake v3.7.2 installed to cmake/cmake-3.7.2-Linux-x86_64
-- Next time you source this script, no download will be neccessary
$ which cmake
/home/[...]/HermitCore/cmake/cmake-3.7.2-Linux-x86_64/bin/cmake
```

## Directory structure

```
cmake/
├── golang
│   ├── CMakeDetermineGoCompiler.cmake
│   ├── CMakeGoCompiler.cmake.in
│   ├── CMakeGoInformation.cmake
│   └── CMakeTestGoCompiler.cmake
├── HermitCore-Application.cmake
├── HermitCore.cmake
├── HermitCore-Paths.cmake
├── HermitCore-Toolchain-x86.cmake
├── HermitCore-Utils.cmake
├── local-cmake.sh
└── README.md
```

## Additional language support

Currently, HermitCore supports `C, C++, Fortran, Go` through Cmake. While the
former are supported and detected by CMake out-of-the-box, Go support has been
added manually for HermitCore.

Adding a new language requires you to provide CMake hints where to find the
toolchain and then how to compile and link binaries. The Go support in
`cmake/golang` may serve as an example on how to add a new language.

To finally enable the language it has to be added to CMake's module path in
`cmake/HermitCore.cmake`:

```
# scripts to detect HermitCore Go compiler
list(APPEND CMAKE_MODULE_PATH ${CMAKE_CURRENT_LIST_DIR}/golang/)

# scripts to detect new language
list(APPEND CMAKE_MODULE_PATH ${CMAKE_CURRENT_LIST_DIR}/new-language/)
```


## User applications

You just have to include `HermitCore-Application.cmake` before the `project()`
command in your `CMakeLists.txt` in order to build applications for HermitCore.
For configuration parameters of the project, please consult the `README.md` in
the root of this repository.
