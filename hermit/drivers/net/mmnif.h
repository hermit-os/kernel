/* 
 * Copyright 2011 Carl-Benedikt Krueger, Chair for Operating Systems,
 *                                       RWTH Aachen University
 *
 * This software is available to you under a choice of one of two
 * licenses.  You may choose to be licensed under the terms of the GNU
 * General Public License (GPL) Version 2 (https://www.gnu.org/licenses/gpl-2.0.txt)
 * or the BSD license below:
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

#ifndef __MMNIF_H__
#define __MMNIF_H__

#include <hermit/stddef.h>

#include <lwip/err.h>
#include <lwip/netif.h>		/* lwip netif */
#include <lwip/sockets.h>

#define AF_MMNIF_NET				0x42

#if 0
#define MMNIF_AUTOACTIVATE_FAST_SOCKETS		LWIP_SOCKET
#else
#define MMNIF_AUTOACTIVATE_FAST_SOCKETS		0
#endif

#if MMNIF_AUTOACTIVATE_FAST_SOCKETS

int mmnif_socket(int domain, int type, int protocol);
int mmnif_send(int s, void *data, size_t size, int flags);
int mmnif_recv(int s, void *data, uint32_t len, int flags);
int mmnif_accept(int s, struct sockaddr *addr, socklen_t * addrlen);
int mmnif_connect(int s, const struct sockaddr *name, socklen_t namelen);
int mmnif_listen(int s, int backlog);
int mmnif_bind(int s, const struct sockaddr *name, socklen_t namelen);
int mmnif_closesocket(int s);
int mmnif_getsockopt (int s, int level, int optname, void *optval, socklen_t *optlen);
int mmnif_setsockopt (int s, int level, int optname, const void *optval, socklen_t optlen);

#undef accept
#define accept(a,b,c)         mmnif_accept(a,b,c)
#undef closesocket
#define closesocket(s)        mmnif_closesocket(s)
#undef connect
#define connect(a,b,c)        mmnif_connect(a,b,c)
#undef recv
#define recv(a,b,c,d)         mmnif_recv(a,b,c,d)
#undef send
#define send(a,b,c,d)         mmnif_send(a,b,c,d)
#undef socket
#define socket(a,b,c)         mmnif_socket(a,b,c)
#undef bind
#define bind(a,b,c)           mmnif_bind(a,b,c)
#undef listen
#define listen(a,b)           mmnif_listen(a,b)
#undef setsockopt
#define setsockopt(a,b,c,d,e) mmnif_setsockopt(a,b,c,d,e)
#undef select
#endif

err_t mmnif_init(struct netif*);
err_t mmnif_shutdown(void);
int mmnif_worker(void *e);
void mmnif_print_driver_status(void);

#endif
