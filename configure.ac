#                                               -*- Autoconf -*-
# Process this file with autoconf to produce a configure script.

AC_PREREQ(2.69)
AC_INIT(HermitCore, 0.1, slankes@eonerc.rwth-aachen.de, HermitCore, http://www.hermitcore.org)
AC_CHECK_PROGS(QEMU, qemu-system-x86_64)
AC_CONFIG_SRCDIR(kernel/main.c)
AC_CONFIG_HEADERS(include/hermit/config.h)

# Checks for programs.

# Checks for libraries.

# Checks for header files.

# Checks for typedefs, structures, and compiler characteristics.

# Checks for library functions.

# define eduOS specific constants
AC_DEFINE(HAVE_ARCH_MEMSET, 1, Define to use machine specific version of memset)
AC_DEFINE(HAVE_ARCH_MEMCPY, 1, Define to use machine specific version of memcpy)
AC_DEFINE(HAVE_ARCH_STRLEN, 1, Define to use machine specific version of strlen)
AC_DEFINE(HAVE_ARCH_STRCPY, 1, Define to use machine specific version of strcpy)
AC_DEFINE(HAVE_ARCH_STRNCPY, 1, Define to use machine specific version of strncpy)
AC_DEFINE(MAX_ISLE, 8, Define the maximum number of isles / numa nodes)
AC_DEFINE(MAX_FNAME, 128, Define the maximum length of a file name)

CROSSDIR=`pwd`/usr/x86/bin
AC_SUBST(CROSSDIR)

AC_ARG_WITH([mtune],
  [AS_HELP_STRING([--with-mtune=ARCH],
                  [enable tuning for a specific architecture])],
  [if test x"$withval" != x ; then
     ARCH_OPT="-mtune=$withval -march=$withval"
   fi],
  [ARCH_OPT=])
AC_SUBST(ARCH_OPT)

AC_ARG_WITH(max-cores, [AS_HELP_STRING([--with-max-cores=N], [specify the maximum number of cores])])
if test "x$with_max_cores" != x; then
AC_DEFINE_UNQUOTED(MAX_CORES, $with_max_tasks, Define the maximum number of cores)
else
AC_DEFINE(MAX_CORES, 512, Define the maximum number of cores)
fi

AC_ARG_WITH(max-tasks, [AS_HELP_STRING([--with-max-tasks=N], [specify the maximum number of running tasks])])
if test "x$with_max_tasks" != x; then
AC_DEFINE_UNQUOTED(MAX_TASKS, $with_max_tasks, Define the maximum number of running tasks)
else
AC_DEFINE(MAX_TASKS, (MAX_CORES*2+2), Define the maximum number of running tasks)
fi

AC_ARG_WITH(kernel-stack-size, [AS_HELP_STRING([--with-kernel-stack-size=SIZE], [specify the size of the kernel stack])])
if test "x$with_kernel_stack_size" != x; then
AC_DEFINE_UNQUOTED(KERNEL_STACK_SIZE, $with_stack_size, Define the size of the kernel stack)
else
AC_DEFINE(KERNEL_STACK_SIZE, 8192, Define the size of the kernel stack)
fi


AC_ARG_WITH(default-stack-size, [AS_HELP_STRING([--with-default-stack-size=SIZE], [specify the default stack size])])
if test "x$with_default_stack_size" != x; then
AC_DEFINE_UNQUOTED(DEFAULT_STACK_SIZE, $with_default_stack_size, Define the default stack size)
else
AC_DEFINE(DEFAULT_STACK_SIZE, 262144, Define the default stack size)
fi

AC_CONFIG_FILES(Makefile)
AC_OUTPUT
