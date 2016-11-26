// 
// Copyright 2010 Intel Corporation
// 
//    Licensed under the Apache License, Version 2.0 (the "License");
//    you may not use this file except in compliance with the License.
//    You may obtain a copy of the License at
// 
//        http://www.apache.org/licenses/LICENSE-2.0
// 
//    Unless required by applicable law or agreed to in writing, software
//    distributed under the License is distributed on an "AS IS" BASIS,
//    WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//    See the License for the specific language governing permissions and
//    limitations under the License.
//
//    [2010-10-25] added support for non-blocking send/recv operations
//                 - iRCCE_isend(), ..._test(), ..._wait(), ..._push()
//                 - iRCCE_irecv(), ..._test(), ..._wait(), ..._push()
//                 by Carsten Clauss, Chair for Operating Systems,
//                                    RWTH Aachen University
//
//    [2010-11-12] extracted non-blocking code into separate library
//                 by Carsten Scholtes
//
//    [2011-04-19] added wildcard mechanism (iRCCE_ANY_SOURCE) for receiving
//                 a message from an arbitrary remote rank
//                 by Simon Pickartz, Chair for Operating Systems,
//                                    RWTH Aachen University
//
//    [2011-06-27] merged iRCCE_ANY_SOURCE branch with trunk (iRCCE_ANY_LENGTH)
//

#ifndef IRCCE_LIB_H
#define IRCCE_LIB_H

#include "RCCE_lib.h"
#include "iRCCE.h"

#ifdef AIR
#define FPGA_BASE 0xf9000000
#define BACKOFF_MIN 8
#define BACKOFF_MAX 256
extern iRCCE_AIR iRCCE_atomic_inc_regs[];
extern int iRCCE_atomic_alloc_counter;
extern iRCCE_AIR* iRCCE_atomic_barrier[2];
#endif

extern iRCCE_SEND_REQUEST* iRCCE_isend_queue;
extern iRCCE_RECV_REQUEST* iRCCE_irecv_queue[RCCE_MAXNP];
extern iRCCE_RECV_REQUEST* iRCCE_irecv_any_source_queue;
extern int iRCCE_recent_source;
extern int iRCCE_recent_length;

#if defined(_OPENMP) && !defined(__hermit__)
#pragma omp threadprivate (iRCCE_isend_queue, iRCCE_irecv_queue, iRCCE_irecv_any_source_queue, iRCCE_recent_source, iRCCE_recent_length)
#endif

int iRCCE_test_flag(RCCE_FLAG, RCCE_FLAG_STATUS, int *);
int iRCCE_push_ssend_request(iRCCE_SEND_REQUEST *request);
int iRCCE_push_srecv_request(iRCCE_RECV_REQUEST *request);

#endif
