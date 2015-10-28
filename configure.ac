#                                               -*- Autoconf -*-
# Process this file with autoconf to produce a configure script.

AC_PREREQ(2.69)
AC_INIT(HermitCore, 0.1, slankes@eonerc.rwth-aachen.de, HermitCore, http://rwth-os.github.io/HermitCore/)
AC_CHECK_PROGS(QEMU, qemu-system-x86_64)

# Checks for programs.

# Checks for libraries.

# Checks for header files.

# Checks for typedefs, structures, and compiler characteristics.

# Checks for library functions.

cp config/linux_config linux/.config
CROSSDIR=`pwd`/hermit/usr/x86/bin
AC_SUBST(CROSSDIR)

AC_CONFIG_FILES(Makefile)
AC_OUTPUT
