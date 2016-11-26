/* Copyright (C) 2005-2015 Free Software Foundation, Inc.
   Contributed by Jakub Jelinek <jakub@redhat.com>.

   This file is part of the GNU Offloading and Multi Processing Library
   (libgomp).

   Libgomp is free software; you can redistribute it and/or modify it
   under the terms of the GNU General Public License as published by
   the Free Software Foundation; either version 3, or (at your option)
   any later version.

   Libgomp is distributed in the hope that it will be useful, but WITHOUT ANY
   WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS
   FOR A PARTICULAR PURPOSE.  See the GNU General Public License for
   more details.

   Under Section 7 of GPL version 3, you are granted additional
   permissions described in the GCC Runtime Library Exception, version
   3.1, as published by the Free Software Foundation.

   You should have received a copy of the GNU General Public License and
   a copy of the GCC Runtime Library Exception along with this program;
   see the files COPYING3 and COPYING.RUNTIME respectively.  If not, see
   <http://www.gnu.org/licenses/>.  */

/* This file contains prototypes of functions in the external ABI.
   This file is included by files in the testsuite.  */

#ifndef LIBGOMP_F_H
#define LIBGOMP_F_H 1

#include "libgomp.h"

#if (32 == 8) \
    && (8 <= 32)
# define OMP_LOCK_DIRECT
typedef omp_lock_t *omp_lock_arg_t;
# define omp_lock_arg(arg) (arg)
#else
typedef union { omp_lock_t *lock; uint64_t u; } *omp_lock_arg_t;
# define omp_lock_arg(arg) ((arg)->lock)
# endif

#if (48 == 8) \
    && (8 <= 48)
# define OMP_NEST_LOCK_DIRECT
typedef omp_nest_lock_t *omp_nest_lock_arg_t;
# define omp_nest_lock_arg(arg) (arg)
#else
typedef union { omp_nest_lock_t *lock; uint64_t u; } *omp_nest_lock_arg_t;
# define omp_nest_lock_arg(arg) ((arg)->lock)
# endif

#if (40 == 8) \
    && (8 <= 40)
# define OMP_LOCK_25_DIRECT
typedef omp_lock_25_t *omp_lock_25_arg_t;
# define omp_lock_25_arg(arg) (arg)
#else
typedef union { omp_lock_25_t *lock; uint64_t u; } *omp_lock_25_arg_t;
# define omp_lock_25_arg(arg) ((arg)->lock)
# endif

#if (48 == 8) \
    && (8 <= 48)
# define OMP_NEST_LOCK_25_DIRECT
typedef omp_nest_lock_25_t *omp_nest_lock_25_arg_t;
# define omp_nest_lock_25_arg(arg) (arg)
#else
typedef union { omp_nest_lock_25_t *lock; uint64_t u; } *omp_nest_lock_25_arg_t;
# define omp_nest_lock_25_arg(arg) ((arg)->lock)
# endif

#ifndef __hermit__
static inline void
omp_check_defines (void)
{
  char test[(32 != sizeof (omp_lock_t)
	     || 8 != __alignof (omp_lock_t)
	     || 48 != sizeof (omp_nest_lock_t)
	     || 8 != __alignof (omp_nest_lock_t)
	     || 8 != sizeof (*(omp_lock_arg_t) 0)
	     || 8 != sizeof (*(omp_nest_lock_arg_t) 0))
	    ? -1 : 1] __attribute__ ((__unused__));
  char test2[(40 != sizeof (omp_lock_25_t)
	     || 8 != __alignof (omp_lock_25_t)
	     || 48 != sizeof (omp_nest_lock_25_t)
	     || 8 != __alignof (omp_nest_lock_25_t)
	     || 8 != sizeof (*(omp_lock_25_arg_t) 0)
	     || 8
		!= sizeof (*(omp_nest_lock_25_arg_t) 0))
	    ? -1 : 1] __attribute__ ((__unused__));
}
#endif

#endif /* LIBGOMP_F_H */
