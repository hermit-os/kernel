/* 
 * Written by the Chair for Operating Systems, RWTH Aachen University
 * 
 * NO Copyright (C) 2010-2011, Stefan Lankes
 * consider these trivial macros to be public domain.
 * 
 * These functions are distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 */

#ifndef __NETINET_IN_H__
#define __NETINET_IN_H__

#include <stddef.h>
#include <sys/types.h>

#ifdef __cplusplus
{
#endif

#if 0
struct in_addr {
  uint32_t s_addr;
};
#endif

/** 255.255.255.255 */
#define IPADDR_NONE         ((uint32_t)0xffffffffUL)
/** 127.0.0.1 */
#define IPADDR_LOOPBACK     ((uint32_t)0x7f000001UL)
/** 0.0.0.0 */
#define IPADDR_ANY          ((uint32_t)0x00000000UL)
/** 255.255.255.255 */
#define IPADDR_BROADCAST    ((uint32_t)0xffffffffUL)

/** 255.255.255.255 */
#define INADDR_NONE         IPADDR_NONE
/** 127.0.0.1 */
#define INADDR_LOOPBACK     IPADDR_LOOPBACK
/** 0.0.0.0 */
#define INADDR_ANY          IPADDR_ANY
/** 255.255.255.255 */
#define INADDR_BROADCAST    IPADDR_BROADCAST

#ifdef __cplusplus
}
#endif

#endif /* __NETINET_IN_H__ */
