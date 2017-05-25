#!/bin/bash

# which version to fetch
MAJOR="3.7"
MINOR="2"
PLATFORM="Linux-x86_64"

# assemble url for desired version
URL="https://cmake.org/files/v${MAJOR}/cmake-${MAJOR}.${MINOR}-${PLATFORM}.tar.gz"

ARCHIVE="$(basename ${URL})"
DIR="$(basename ${ARCHIVE} .tar.gz)"


relpath() {
	# workaround because Ubuntu seems to use an ancient realpath version
	# https://stackoverflow.com/questions/2564634/convert-absolute-path-into-relative-path-given-a-current-directory-using-bash#comment12808306_7305217
	python -c "import os.path; print(os.path.relpath('${2:-$PWD}','$1'))";
}

HERMIT_TOP="$(git rev-parse --show-toplevel)"
HERMIT_CMAKE="${HERMIT_TOP}/cmake"
CMAKE_DIR="${HERMIT_CMAKE}/${DIR}"
CMAKE_DIR_REL="$(relpath ${HERMIT_TOP} ${CMAKE_DIR})"

# make sure we're sourced, not executed
if [ "$0" = "$BASH_SOURCE" ]
then
	echo "You have to source this script:"
	echo "\$ . $0"
	exit
fi

# quit if already in path
echo "$PATH" | grep "${CMAKE_DIR_REL}" &>/dev/null && return

# check if already installed
if which cmake &> /dev/null ; then
	if cmake --version | grep "cmake version ${MAJOR}.${MINOR}" &> /dev/null;	 then
		echo "You already have CMake ${MAJOR}.${MINOR}"
		return
	fi
fi

if [ ! -d "${CMAKE_DIR}" ]
then
	echo "-- Downloading CMake"
	wget "${URL}" -O "${ARCHIVE}" ||
		(echo "Cannot download CMake"; return)

	echo "-- Unpacking CMake"
	tar -C "${HERMIT_CMAKE}" -xf "${ARCHIVE}" ||
		(echo "Cannot unpack CMake archive"; return)

	# delete temporary archive again
	rm -f "${ARCHIVE}"

	# add cmake dir to gitignore
	GITIGNORE="${HERMIT_TOP}/.gitignore"
	if ! grep "${CMAKE_DIR_REL}" "${GITIGNORE}" &>/dev/null
	then
		echo "${CMAKE_DIR_REL}/*" >> "${GITIGNORE}"
	fi

	echo "-- Local CMake v${MAJOR}.${MINOR} installed to ${CMAKE_DIR_REL}"
	echo "-- Next time you source this script, no download will be neccessary"
fi

export PATH="${CMAKE_DIR}/bin:${PATH}"
