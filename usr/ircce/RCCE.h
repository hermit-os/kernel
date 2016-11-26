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
//                 - RCCE_isend(), ..._test(), ..._wait(), ..._push()
//                 - RCCE_irecv(), ..._test(), ..._wait(), ..._push()
//                 by Carsten Clauss, Chair for Operating Systems,
//                                    RWTH Aachen University
//
//    [2012-09-10] added support for "tagged" flags
//                 - RCCE_send_tagged(), RCCE_recv_tagged(), RCCE_recv_probe_tagged()
//                 by Carsten Clauss, Chair for Operating Systems,
//                                    RWTH Aachen University
//
//    [2015-10-18] port (i)RCCE to "HermitCore"
//                 by Stefan Lankes, Institute for Automation of Complex Power Systems
//                                   RWTH Aachen University

#ifndef RCCE_H
#define RCCE_H

#include <stdlib.h>
#include <stdio.h>

#ifdef __hermit__
#define SCC
#define COPPERRIDGE
#define USE_REMOTE_PUT_LOCAL_GET
#define USE_PROBE_FLAGS
#undef SHMADD
#endif

#define _RCCE "1.0.13 release"
// #define USE_BYTE_FLAGS
// #define USE_FLAG_EXPERIMENTAL
// little trick to allow the application to be called "RCCE_APP" under
// OpenMP, and "main" otherwise 

#define ABS(x) ((x > 0)?x:-x)

#if !defined(_OPENMP) || defined(__hermit__)
  #define RCCE_APP main
#endif

// modify next line for Intel BareMetal, which supports stdout, but not stdferr 
#define STDERR                             stdout

#ifdef __hermit__
#define LOG2_LINE_SIZE                     6
#else
#define LOG2_LINE_SIZE                     5
#endif
#define RCCE_LINE_SIZE                     (1<<LOG2_LINE_SIZE)
// RCCE_BUFF_SIZE_MAX is space per UE, which is half of the space per tile 
#ifdef __hermit__
#define RCCE_BUFF_SIZE_MAX                 (64*1024)
#else
#define RCCE_BUFF_SIZE_MAX                 (1<<13)
#endif

#ifdef SHMADD
//64MB
//#define RCCE_SHM_SIZE_MAX                0x4000000 
// 128MB
//#define RCCE_SHM_SIZE_MAX                0x8000000 
// 256MB
//#define RCCE_SHM_SIZE_MAX                0x10000000 
// 512MB
#define RCCE_SHM_SIZE_MAX                  0x20000000 
// 960MB
//#define RCCE_SHM_SIZE_MAX                0x3C000000 
#else
  #ifndef SCC_COUPLED_SYSTEMS
  // 64MB
  #define RCCE_SHM_SIZE_MAX                  (1<<26)
  #else
  // In Coupled Mode only 4MB
  #define RCCE_SHM_SIZE_MAX                  (1<<22)
  #endif
#endif

#ifdef __hermit__
#define RCCE_MAX_BOARDS			   1
#define RCCE_MAXNP_PER_BOARD               8
#else
#define RCCE_MAX_BOARDS                    2 /* allow up to 2 SCC boards for now */
#define RCCE_MAXNP_PER_BOARD               48
#endif
#define RCCE_MAXNP                         (RCCE_MAX_BOARDS * RCCE_MAXNP_PER_BOARD)
#define RCCE_SUCCESS                       0
#define RCCE_PENDING                       -1
#define RCCE_RESERVED                      -2
#define RCCE_REJECTED                      -3
#define RCCE_ERROR_BASE                    1234321
#define RCCE_ERROR_TARGET                  (RCCE_ERROR_BASE +  1)
#define RCCE_ERROR_SOURCE                  (RCCE_ERROR_BASE +  2)
#define RCCE_ERROR_ID                      (RCCE_ERROR_BASE +  3)
#define RCCE_ERROR_MESSAGE_LENGTH          (RCCE_ERROR_BASE +  4)
#define RCCE_ERROR_FLAG_UNDEFINED          (RCCE_ERROR_BASE +  5)
#define RCCE_ERROR_NUM_UES                 (RCCE_ERROR_BASE +  6)
#define RCCE_ERROR_DATA_OVERLAP            (RCCE_ERROR_BASE +  7)
#define RCCE_ERROR_ALIGNMENT               (RCCE_ERROR_BASE +  8)
#define RCCE_ERROR_DEBUG_FLAG              (RCCE_ERROR_BASE +  9)
#define RCCE_ERROR_FLAG_NOT_IN_COMM_BUFFER (RCCE_ERROR_BASE + 10)
#define RCCE_ERROR_FLAG_STATUS_UNDEFINED   (RCCE_ERROR_BASE + 11)
#define RCCE_ERROR_FLAG_NOT_ALLOCATED      (RCCE_ERROR_BASE + 12)
#define RCCE_ERROR_VAL_UNDEFINED           (RCCE_ERROR_BASE + 13)
#define RCCE_ERROR_INVALID_ERROR_CODE      (RCCE_ERROR_BASE + 14)
#define RCCE_ERROR_RPC_NOT_ALLOCATED       (RCCE_ERROR_BASE + 15)
#define RCCE_ERROR_RPC_INTERNAL            (RCCE_ERROR_BASE + 16)
#define RCCE_ERROR_MULTIPLE_RPC_REQUESTS   (RCCE_ERROR_BASE + 17)
#define RCCE_ERROR_FDIVIDER                (RCCE_ERROR_BASE + 18)
#define RCCE_ERROR_FREQUENCY_EXCEEDED      (RCCE_ERROR_BASE + 19)
#define RCCE_ERROR_NO_ACTIVE_RPC_REQUEST   (RCCE_ERROR_BASE + 20)
#define RCCE_ERROR_STALE_RPC_REQUEST       (RCCE_ERROR_BASE + 21)
#define RCCE_ERROR_COMM_UNDEFINED          (RCCE_ERROR_BASE + 22)
#define RCCE_ERROR_ILLEGAL_OP              (RCCE_ERROR_BASE + 23)
#define RCCE_ERROR_ILLEGAL_TYPE            (RCCE_ERROR_BASE + 24)
#define RCCE_ERROR_MALLOC                  (RCCE_ERROR_BASE + 25)
#define RCCE_ERROR_COMM_INITIALIZED        (RCCE_ERROR_BASE + 26)
#define RCCE_ERROR_CORE_NOT_IN_HOSTFILE    (RCCE_ERROR_BASE + 27)
#define RCCE_ERROR_NO_MULTICAST_SUPPORT    (RCCE_ERROR_BASE + 28)
#define RCCE_MAX_ERROR_STRING              45

#define RCCE_DEBUG_ALL                     111111
#define RCCE_DEBUG_SYNCH                   111444
#define RCCE_DEBUG_COMM                    111555
#define RCCE_DEBUG_RPC                     111666
#define RCCE_DEBUG_DEBUG                   111888

#define RCCE_FLAG_SET                      1
#define RCCE_FLAG_UNSET                    0

#define RCCE_NUM_OPS                       4
#define RCCE_OP_BASE                       23232323
#define RCCE_SUM                           (RCCE_OP_BASE)
#define RCCE_MIN                           (RCCE_OP_BASE+1)
#define RCCE_MAX                           (RCCE_OP_BASE+2)
#define RCCE_PROD                          (RCCE_OP_BASE+3)

#define RCCE_TYPE_BASE                     63636363
#define RCCE_INT                           (RCCE_TYPE_BASE)
#define RCCE_LONG                          (RCCE_TYPE_BASE+1)
#define RCCE_FLOAT                         (RCCE_TYPE_BASE+2)
#define RCCE_DOUBLE                        (RCCE_TYPE_BASE+3)

// MPB pointer type
typedef volatile unsigned char* t_vcharp;

#if (defined(SINGLEBITFLAGS) || defined(USE_BYTE_FLAGS)) && !defined(USE_FLAG_EXPERIMENTAL)
typedef struct {
   int  location;      /* location of bit within line (0-255)  */
   t_vcharp flag_addr; /* address of byte containing flag inside cache line */
   t_vcharp line_address; /* start of cache line containing flag  */
}  RCCE_FLAG;
#else
#ifdef USE_FLAG_EXPERIMENTAL
typedef volatile unsigned char *RCCE_FLAG;
#else
typedef volatile ssize_t *RCCE_FLAG;
#endif
#endif

#ifdef USE_FLAG_EXPERIMENTAL
typedef unsigned char RCCE_FLAG_STATUS;
#else
typedef ssize_t RCCE_FLAG_STATUS;
#endif

typedef struct {
  int size;
  int my_rank;
  int initialized;
  int member[RCCE_MAXNP];
#ifdef USE_FAT_BARRIER 
  RCCE_FLAG gather[RCCE_MAXNP];
#else
  RCCE_FLAG gather;
#endif
  RCCE_FLAG release;  
  volatile int cycle;
  volatile int count;
  int step;
  int label;
} RCCE_COMM;

typedef struct _RCCE_SEND_REQUEST {
  char *privbuf;    // source buffer in local private memory (send buffer)
  t_vcharp combuf;  // intermediate buffer in MPB
  size_t chunk;     // size of MPB available for this message (bytes)
  RCCE_FLAG *ready; // flag indicating whether receiver is ready
  RCCE_FLAG *sent;  // flag indicating whether message has been sent by source
  size_t size;      // size of message (bytes)
  int dest;         // UE that will receive the message

  int copy;         // set to 0 for synchronization only (no copying/sending)
  void* tag;        // additional tag?
  int len;          // length of additional tag
  RCCE_FLAG *probe; // flag for probing for incoming messages

  size_t wsize;     // offset within send buffer when putting in "chunk" bytes
  size_t remainder; // bytes remaining to be sent
  size_t nbytes;    // number of bytes to be sent in single RCCE_put call
  char *bufptr;     // running pointer inside privbuf for current location

  int label;        // jump/goto label for the reentrance of the respective poll function
  int finished;     // flag that indicates whether the request has already been finished

  struct _RCCE_SEND_REQUEST *next;
} RCCE_SEND_REQUEST;

typedef struct _RCCE_RECV_REQUEST {
  char *privbuf;    // source buffer in local private memory (send buffer)
  t_vcharp combuf;  // intermediate buffer in MPB
  size_t chunk;     // size of MPB available for this message (bytes)
  RCCE_FLAG *ready; // flag indicating whether receiver is ready
  RCCE_FLAG *sent;  // flag indicating whether message has been sent by source
  size_t size;      // size of message (bytes)
  int source;       // UE that will send the message

  int copy;         // set to 0 for cancel function
  void* tag;        // additional tag?
  int len;          // length of additional tag
  RCCE_FLAG *probe;  // flag for probing for incoming messages

  size_t wsize;     // offset within send buffer when putting in "chunk" bytes
  size_t remainder; // bytes remaining to be sent
  size_t nbytes;    // number of bytes to be sent in single RCCE_put call
  char *bufptr;     // running pointer inside privbuf for current location

  int label;        // jump/goto label for the reentrance of the respective poll function
  int finished;     // flag that indicates whether the request has already been finished

  struct _RCCE_RECV_REQUEST *next;
} RCCE_RECV_REQUEST;

typedef struct tree_s {
  int parent; // UE of parent
  int num_children;
  int child[RCCE_MAXNP]; // UEs of children
} tree_t;

#ifdef RC_POWER_MANAGEMENT
typedef struct{
    int release;
    int old_voltage_level;
    int new_voltage_level;
    int old_frequency_divider;
    int new_frequency_divider;
    long long start_cycle;
  } RCCE_REQUEST;
int RCCE_power_domain(void);
int RCCE_iset_power(int, RCCE_REQUEST *, int *, int *);
int RCCE_wait_power(RCCE_REQUEST *);
int RCCE_set_frequency_divider(int, int *);
int RCCE_power_domain_master(void);
int RCCE_power_domain_size(void);
#endif  

int    RCCE_init(int *, char***);
int    RCCE_finalize(void);
double RCCE_wtime(void);
int    RCCE_ue(void);
int    RCCE_num_ues(void);
#ifdef SCC_COUPLED_SYSTEMS
int RCCE_dev(void);
int RCCE_dev_ue(void);
int RCCE_num_dev(void);
int RCCE_num_ues_dev(int);
int RCCE_ue_to_dev(int);
#endif
#ifdef GORY
t_vcharp RCCE_malloc(size_t);
t_vcharp RCCE_malloc_request(size_t, size_t *);
t_vcharp RCCE_palloc(size_t,int);
void   RCCE_free(t_vcharp);
int    RCCE_put(t_vcharp, t_vcharp, int, int);
int    RCCE_get(t_vcharp, t_vcharp, int, int);
int    RCCE_wait_until(RCCE_FLAG, RCCE_FLAG_STATUS);
int    RCCE_test_flag(RCCE_FLAG, RCCE_FLAG_STATUS, int *);
int    RCCE_flag_alloc(RCCE_FLAG *);
int    RCCE_flag_free(RCCE_FLAG *);
int    RCCE_flag_write(RCCE_FLAG *, RCCE_FLAG_STATUS, int);
int    RCCE_flag_read(RCCE_FLAG, RCCE_FLAG_STATUS *, int);
int    RCCE_flag_write_tagged(RCCE_FLAG *, RCCE_FLAG_STATUS, int, char*, int); 
int    RCCE_flag_read_tagged(RCCE_FLAG, RCCE_FLAG_STATUS *, int, char*, int);
int    RCCE_send(char *, t_vcharp, size_t, RCCE_FLAG *, RCCE_FLAG *, size_t, int);
int    RCCE_recv(char *, t_vcharp, size_t, RCCE_FLAG *, RCCE_FLAG *, size_t, int, RCCE_FLAG *);
int    RCCE_recv_test(char *, t_vcharp, size_t, RCCE_FLAG *, RCCE_FLAG *, size_t, int, int *, RCCE_FLAG *);
#ifdef USE_FLAG_EXPERIMENTAL
int    RCCE_put_flag(t_vcharp, t_vcharp, int, int);
int    RCCE_get_flag(t_vcharp, t_vcharp, int, int);
#endif
#else
// standard non-gory functions:

t_vcharp RCCE_malloc(size_t);

int    RCCE_flag_write(RCCE_FLAG *, RCCE_FLAG_STATUS, int);
int    RCCE_flag_read(RCCE_FLAG, RCCE_FLAG_STATUS *, int);

int    RCCE_send(char *, size_t, int);
int    RCCE_recv(char *, size_t, int);
int    RCCE_recv_test(char *, size_t, int, int *);
int    RCCE_send_pipe(char *, size_t, int);
int    RCCE_recv_pipe(char *, size_t, int);
int    RCCE_send_mcast(char *, size_t);
int    RCCE_recv_mcast(char *, size_t, int);
int    RCCE_send_tagged(char *, size_t, int, void *, int);
int    RCCE_recv_tagged(char *, size_t, int, void *, int);
int    RCCE_recv_probe_tagged(int, int *, t_vcharp *, void *, int);
int    RCCE_allreduce(char *, char *, int, int, int, RCCE_COMM);
int    RCCE_reduce(char *, char *, int, int, int, int, RCCE_COMM);
int    RCCE_bcast(char *, size_t, int, RCCE_COMM);
int    RCCE_recv_probe(int, int *, t_vcharp *);
int    RCCE_recv_cancel(size_t, int);
int    RCCE_isend(char *, size_t, int, RCCE_SEND_REQUEST *);
int    RCCE_isend_test(RCCE_SEND_REQUEST *, int *);
int    RCCE_isend_wait(RCCE_SEND_REQUEST *);
int    RCCE_isend_push(int);
int    RCCE_irecv(char *, size_t, int, RCCE_RECV_REQUEST *);
int    RCCE_irecv_test(RCCE_RECV_REQUEST *, int *);
int    RCCE_irecv_wait(RCCE_RECV_REQUEST *);
int    RCCE_irecv_push(int);

#endif
t_vcharp RCCE_shmalloc(size_t);
void     RCCE_shfree(t_vcharp);
void     RCCE_shflush(void);
t_vcharp RCCE_shrealloc(t_vcharp, size_t);

// LfBS-customized functions:
void*  RCCE_memcpy_get(void *, const void *, size_t);
void*  RCCE_memcpy_put(void *, const void *, size_t);
#define RCCE_memcpy(a,b,c) RCCE_memcpy_put(a,b,c)

int    RCCE_comm_split(int (*)(int, void *), void *, RCCE_COMM *);
int    RCCE_comm_free(RCCE_COMM *);
int    RCCE_comm_size(RCCE_COMM, int *);
int    RCCE_comm_rank(RCCE_COMM, int *);
void   RCCE_fence(void);
int    RCCE_barrier(RCCE_COMM *);
int    RCCE_tree_init(RCCE_COMM *, tree_t *, int);
int    RCCE_tree_barrier(RCCE_COMM *, tree_t *);
int    RCCE_tournament_barrier(RCCE_COMM *);
int    RCCE_tournament_fixed_barrier(RCCE_COMM *);
int    RCCE_dissemination_barrier(RCCE_COMM *);
int    RCCE_TNS_barrier(RCCE_COMM *);
int    RCCE_AIR_barrier(RCCE_COMM *);
int    RCCE_AIR_barrier2(RCCE_COMM *);
int    RCCE_nb_barrier(RCCE_COMM *);
int    RCCE_nb_TNS_barrier(RCCE_COMM *);
int    RCCE_nb_AIR_barrier(RCCE_COMM *);
int    RCCE_error_string(int, char *, int *);
int    RCCE_debug_set(int);
int    RCCE_debug_unset(int);

extern RCCE_COMM    RCCE_COMM_WORLD;
#ifdef RC_POWER_MANAGEMENT
extern RCCE_COMM    RCCE_P_COMM;
#define RCCE_POWER_DEFAULT -99999
#endif

#if defined(_OPENMP) && !defined(__hermit__)
#pragma omp threadprivate (RCCE_COMM_WORLD)
#ifdef RC_POWER_MANAGEMENT
#pragma omp threadprivate (RCCE_P_COMM)
#endif
#endif

#endif
