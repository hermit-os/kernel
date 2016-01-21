/*
 * Copyright (c) 2015, Stefan Lankes, RWTH Aachen University
 * All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions are met:
 *    * Redistributions of source code must retain the above copyright
 *      notice, this list of conditions and the following disclaimer.
 *    * Redistributions in binary form must reproduce the above copyright
 *      notice, this list of conditions and the following disclaimer in the
 *      documentation and/or other materials provided with the distribution.
 *    * Neither the name of the University nor the names of its contributors
 *      may be used to endorse or promote products derived from this
 *      software without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED
 * WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
 * DISCLAIMED. IN NO EVENT SHALL THE REGENTS OR CONTRIBUTORS BE LIABLE FOR ANY
 * DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES
 * (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES;
 * LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND
 * ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
 * (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS
 * SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 */

#ifndef __SYS_IPC_H__
#define __SYS_IPC_H__

#include <stdlib.h>

#ifdef __cplusplus
extern "C" {
#endif

#ifdef __KERNEL__
typedef long key_t;
#endif

#define IPC_PRIVATE ((key_t) 0) 

/* Obsolete, used only for backwards compatibility and libc5 compiles */
struct ipc_perm
{
	key_t	key;
#if 0
        __kernel_uid_t  uid;
        __kernel_gid_t  gid;
        __kernel_uid_t  cuid;
        __kernel_gid_t  cgid;
        __kernel_mode_t mode; 
        unsigned short  seq;
#endif
};

/* resource get request flags */
#define IPC_CREAT	00001000	/* create if key is nonexistent */
#define IPC_EXCL	00002000	/* fail if key exists */
#define IPC_NOWAIT	00004000	/* return error on wait */

/* 
 * Control commands used with semctl, msgctl and shmctl 
 * see also specific commands in shm.h
 */
#define IPC_RMID 0	/* remove resource */
#define IPC_SET  1	/* set ipc_perm options */
#define IPC_STAT 2	/* get ipc_perm options */
#define IPC_INFO 3	/* see ipcs */

#ifdef __cplusplus
}
#endif

#endif
