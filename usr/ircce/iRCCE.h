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
//                 by Carsten Scholtes, University of Bayreuth
//
//    [2010-12-09] added functions for a convenient handling of multiple
//                 pending non-blocking requests
//                 by Jacek Galowicz, Chair for Operating Systems
//                                    RWTH Aachen University
//
//    [2011-04-19] added wildcard mechanism (iRCCE_ANY_SOURCE) for receiving
//                 a message from an arbitrary remote rank
//                 by Simon Pickartz, Chair for Operating Systems,
//                                    RWTH Aachen University
//
//    [2011-06-16] iRCCE_ANY_LENGTH wildcard mechanism can only be used in
//                 the SINGLEBITFLAGS=0 case (-> bigflags must be enabled!)
//
//    [2011-06-27] merged iRCCE_ANY_SOURCE branch with trunk (iRCCE_ANY_LENGTH)
//
//    [2011-11-03] - renamed blocking (pipelined) send/recv functions to
//                   iRCCE_ssend() / iRCCE_srecv() (strictly synchronous!)
//                 - added non-blocking by synchronous send/recv functions:
//                   iRCCE_issend() / iRCCE_isrecv()
//                 by Carsten Clauss, Chair for Operating Systems,
//                                    RWTH Aachen University
//
//    [2012-10-29] - added functions for handling "Tagged Flags"
//                   iRCCE_flag_read/write_tagged(), iRCCE_test/wait_tagged()
//                 by Carsten Clauss, Chair for Operating Systems,
//                                    RWTH Aachen University
//
//    [2015-10-18] port (i)RCCE to "HermitCore"
//                 by Stefan Lankes, Institute for Automation of Complex Power Systems
//                                   RWTH Aachen University

#ifndef IRCCE_H
#define IRCCE_H

#include "RCCE.h"

#define iRCCE_VERSION "2.0"
#define iRCCE_FLAIR

#define iRCCE_SUCCESS  RCCE_SUCCESS
#define iRCCE_ERROR        -1
#define iRCCE_PENDING      -2
#define iRCCE_RESERVED     -3
#define iRCCE_NOT_ENQUEUED -4

#if !defined(SINGLEBITFLAGS) && !defined(RCCE_VERSION)
#define _iRCCE_ANY_LENGTH_
extern const int iRCCE_ANY_LENGTH;
#endif

#if !defined(SINGLEBITFLAGS)
#if defined(_OPENMP) && !defined(__hermit__)
#define iRCCE_MAX_TAGGED_LEN (RCCE_LINE_SIZE - 2 * sizeof(int))
#else
#define iRCCE_MAX_TAGGED_LEN (RCCE_LINE_SIZE - sizeof(int))
#endif
#endif

extern const int iRCCE_ANY_SOURCE;

typedef struct _iRCCE_SEND_REQUEST {
  char *privbuf;    // source buffer in local private memory (send buffer)
  t_vcharp combuf;  // intermediate buffer in MPB
  size_t chunk;     // size of MPB available for this message (bytes)
  size_t subchunk1; // sub-chunks for the pipelined message transfe
  size_t subchunk2;
  RCCE_FLAG *ready; // flag indicating whether receiver is ready
  RCCE_FLAG *sent;  // flag indicating whether message has been sent by source
  RCCE_FLAG_STATUS flag_set_value; // used for iRCCE_ANY_LENGTH wildcard
  size_t size;      // size of message (bytes)
  int dest;         // UE that will receive the message
  int sync;         // flag indicating whether send is synchronous or not

  size_t wsize;     // offset within send buffer when putting in "chunk" bytes
  size_t remainder;  // bytes remaining to be sent
  size_t nbytes;    // number of bytes to be sent in single RCCE_put call
  char *bufptr;     // running pointer inside privbuf for current location

  int label;        // jump/goto label for the reentrance of the respective poll function
  int finished;     // flag that indicates whether the request has already been finished

  struct _iRCCE_SEND_REQUEST *next;
} iRCCE_SEND_REQUEST;


typedef struct _iRCCE_RECV_REQUEST {
  char *privbuf;    // source buffer in local private memory (send buffer)
  t_vcharp combuf;  // intermediate buffer in MPB
  size_t chunk;     // size of MPB available for this message (bytes)
  size_t subchunk1; // sub-chunks for the pipelined message transfe
  size_t subchunk2;
  RCCE_FLAG *ready; // flag indicating whether receiver is ready
  RCCE_FLAG *sent;  // flag indicating whether message has been sent by source
  RCCE_FLAG_STATUS flag_set_value; // used for iRCCE_ANY_LENGTH wildcard
  size_t size;      // size of message (bytes)
  int source;       // UE that will send the message
  int sync;         // flag indicating whether recv is synchronous or not

  size_t wsize;     // offset within send buffer when putting in "chunk" bytes
  size_t remainder; // bytes remaining to be sent
  size_t nbytes;    // number of bytes to be sent in single RCCE_put call
  char *bufptr;     // running pointer inside privbuf for current location

  int label;        // jump/goto label for the reentrance of the respective poll function
  int finished;     // flag that indicates whether the request has already been finished
  int started;      // flag that indicates whether message parts have already been received

  struct _iRCCE_RECV_REQUEST *next;
} iRCCE_RECV_REQUEST;

#define iRCCE_WAIT_LIST_RECV_TYPE 0
#define iRCCE_WAIT_LIST_SEND_TYPE 1

typedef struct _iRCCE_WAIT_LISTELEM {
	int type;
	struct _iRCCE_WAIT_LISTELEM * next;
	void * req;
} iRCCE_WAIT_LISTELEM;

typedef struct _iRCCE_WAIT_LIST {
	iRCCE_WAIT_LISTELEM * first;
	iRCCE_WAIT_LISTELEM * last;
} iRCCE_WAIT_LIST;

#ifdef AIR
typedef volatile struct _iRCCE_AIR {
#if !defined(_OPENMP) || defined(__hermit__)
        int * counter;
        int * init;
#else
        int counter;
        int init;
#endif
} iRCCE_AIR;
#endif

///////////////////////////////////////////////////////////////
//
//                       THE iRCCE API:
//
//  Initialize function:
int   iRCCE_init(void);
//
//  Non-blocking send/recv functions:
int   iRCCE_isend(char *, ssize_t, int, iRCCE_SEND_REQUEST *);
int   iRCCE_isend_test(iRCCE_SEND_REQUEST *, int *);
int   iRCCE_isend_wait(iRCCE_SEND_REQUEST *);
int   iRCCE_isend_push(void);
int   iRCCE_irecv(char *, ssize_t, int, iRCCE_RECV_REQUEST *);
int   iRCCE_irecv_test(iRCCE_RECV_REQUEST *, int *);
int   iRCCE_irecv_wait(iRCCE_RECV_REQUEST *);
int   iRCCE_irecv_push(void);
//
//  Pipelined send/recv functions: (syncronous and blocking)
int   iRCCE_ssend(char *, ssize_t, int);
int   iRCCE_srecv(char *, ssize_t, int);
int   iRCCE_srecv_test(char *, ssize_t, int, int*);
//
//  Non-blocking pipelined send/recv functions:
int   iRCCE_issend(char *, ssize_t, int, iRCCE_SEND_REQUEST *);
int   iRCCE_isrecv(char *, ssize_t, int, iRCCE_RECV_REQUEST *);
//
//  SCC-customized put/get and memcpy functions:
int   iRCCE_put(t_vcharp, t_vcharp, int, int);
int   iRCCE_get(t_vcharp, t_vcharp, int, int);
void* iRCCE_memcpy_put(void*, const void*, size_t);
void* iRCCE_memcpy_get(void*, const void*, size_t);
t_vcharp iRCCE_malloc(size_t);
#define iRCCE_memcpy iRCCE_memcpy_put
//
//  Blocking and non-blocking 'probe' functions for incommimg messages:
int   iRCCE_probe(int, int*);
int   iRCCE_iprobe(int, int*, int*);
//
//  Wait/test-all/any functions:
void  iRCCE_init_wait_list(iRCCE_WAIT_LIST*);
void  iRCCE_add_to_wait_list(iRCCE_WAIT_LIST*, iRCCE_SEND_REQUEST *, iRCCE_RECV_REQUEST *);
int   iRCCE_test_all(iRCCE_WAIT_LIST*, int *);
int   iRCCE_wait_all(iRCCE_WAIT_LIST*);
int   iRCCE_test_any(iRCCE_WAIT_LIST*, iRCCE_SEND_REQUEST **, iRCCE_RECV_REQUEST **);
int   iRCCE_wait_any(iRCCE_WAIT_LIST*, iRCCE_SEND_REQUEST **, iRCCE_RECV_REQUEST **);
//
//  Query functions for request handle parameters:
int   iRCCE_get_dest(iRCCE_SEND_REQUEST*);
int   iRCCE_get_source(iRCCE_RECV_REQUEST*);
int   iRCCE_get_size(iRCCE_SEND_REQUEST*, iRCCE_RECV_REQUEST*);
int   iRCCE_get_length(void);
//
//  Cancel functions for yet not started non-blocking requests:
int   iRCCE_isend_cancel(iRCCE_SEND_REQUEST *, int *);
int   iRCCE_irecv_cancel(iRCCE_RECV_REQUEST *, int *);
//
//  Functions for handling tagged flags: (need whole cache line per flag)
#ifndef SINGLEBITFLAGS
int   iRCCE_flag_alloc_tagged(RCCE_FLAG *);
int   iRCCE_flag_write_tagged(RCCE_FLAG *, RCCE_FLAG_STATUS, int, void *, int);
int   iRCCE_flag_read_tagged(RCCE_FLAG, RCCE_FLAG_STATUS *, int, void *, int);
int   iRCCE_wait_tagged(RCCE_FLAG, RCCE_FLAG_STATUS, void *, int);
int   iRCCE_test_tagged(RCCE_FLAG, RCCE_FLAG_STATUS, int *, void *, int);
int   iRCCE_get_max_tagged_len(void);
#endif
//
//  Functions for handling Atomic Increment Registers (AIR):
#ifdef AIR
int   iRCCE_atomic_alloc(iRCCE_AIR **);
int   iRCCE_atomic_inc(iRCCE_AIR*, int*);
int   iRCCE_atomic_read(iRCCE_AIR*, int*);
int   iRCCE_atomic_write(iRCCE_AIR*, int);
#endif
//
//  Improved Collectives:
int   iRCCE_barrier(RCCE_COMM*);
int   iRCCE_bcast(char *, size_t, int, RCCE_COMM);
int   iRCCE_mcast(char *, size_t, int);
int   iRCCE_msend(char *, ssize_t);
int   iRCCE_mrecv(char *, ssize_t, int);
//
//  Functions form the GORY RCCE interface mapped to iRCCE:
t_vcharp iRCCE_malloc(size_t);
int   iRCCE_flag_alloc(RCCE_FLAG *);
int   iRCCE_flag_write(RCCE_FLAG *, RCCE_FLAG_STATUS, int);
int   iRCCE_flag_read(RCCE_FLAG, RCCE_FLAG_STATUS *, int);
int   iRCCE_wait_until(RCCE_FLAG, RCCE_FLAG_STATUS);
//
// Please Note: Since we're running in NON-GORY mode, there are no "free()" functions!
//
///////////////////////////////////////////////////////////////
//
//      Just for convenience:
#if 1
#define RCCE_isend        iRCCE_isend
#define RCCE_isend_test   iRCCE_isend_test
#define RCCE_isend_wait   iRCCE_isend_wait
#define RCCE_isend_push   iRCCE_isend_push
#define RCCE_irecv        iRCCE_irecv
#define RCCE_irecv_test   iRCCE_irecv_test
#define RCCE_irecv_wait   iRCCE_irecv_wait
#define RCCE_irecv_push   iRCCE_irecv_push
#define RCCE_SEND_REQUEST iRCCE_SEND_REQUEST
#define RCCE_RECV_REQUEST iRCCE_RECV_REQUEST
#ifdef _iRCCE_TAGGED_FLAGS_
#define RCCE_flag_write_tagged iRCCE_flag_write_tagged
#define RCCE_flag_read_tagged  iRCCE_flag_read_tagged
#define RCCE_wait_tagged       iRCCE_wait_tagged
#define RCCE_test_tagged       iRCCE_test_tagged
#define RCCE_flag_alloc_tagged iRCCE_flag_alloc_tagged
#define RCCE_flag_free_tagged  iRCCE_flag_free_tagged
#endif
#endif
//
#if 1
#define iRCCE_send        iRCCE_ssend
#define iRCCE_recv        iRCCE_srecv
#define iRCCE_recv_test   iRCCE_srecv_test
#endif
//
#if 1
#define iRCCE_issend_test iRCCE_isend_test
#define iRCCE_issend_wait iRCCE_isend_wait
#define iRCCE_issend_push iRCCE_isend_push
#define iRCCE_isrecv_test iRCCE_irecv_test
#define iRCCE_isrecv_wait iRCCE_irecv_wait
#define iRCCE_isrecv_push iRCCE_irecv_push
#endif
//
///////////////////////////////////////////////////////////////

#endif

