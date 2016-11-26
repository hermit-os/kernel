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
#ifndef RCCE_LIB_H
#define RCCE_LIB_H
#include "RCCE.h"
#if defined(_OPENMP) && !defined(__hermit__)
  #include <omp.h>
#endif
#include <string.h>

//#define AIR

#undef USE_FLAG_EXPERIMENTAL
#undef USE_RCCE_COMM
#undef USE_FAT_BARRIER
#undef USE_PIPELINE_FLAGS
#undef USE_PROBE_FLAGS
#undef USE_TAGGED_FLAGS
#undef USE_TAGGED_FOR_SHORT
#undef USE_REVERTED_FLAGS
#undef USE_REMOTE_PUT_LOCAL_GET
#undef USE_PROBE_FLAGS_SHORTCUT
#define USE_SYNCH_FOR_ZERO_BYTE

// override certain settings for SCC-MPICH:
//#include "scc-mpich-defs.h"

// adjust settings automatically?
#undef AUTO_ADJUST_SETTINGS

////////////////////////////////////////////////////////////////////////////////////////////////
#ifdef AUTO_ADJUST_SETTINGS

#ifdef SINGLEBITFLAGS
#ifdef USE_TAGGED_FLAGS
#warning TAGGED FLAGS CANNOT BE USED WITH SINGLEBITFLAGS! (#undef USE_TAGGED_FLAGS)
#undef USE_TAGGED_FLAGS
#undef USE_TAGGED_FOR_SHORT
#undef USE_PROBE_FLAGS_SHORTCUT
#endif
#ifdef USE_FAT_BARRIER
#warning FAT BARRIER CANNOT BE USED WITH SINGLEBITFLAGS! (#undef USE_FAT_BARRIER)
#undef USE_FAT_BARRIER
#endif
#endif

#ifdef USE_PROBE_FLAGS_SHORTCUT
#ifndef USE_PROBE_FLAGS
#warning THE PROBE FLAGS SHORTCUT REQUIRES PROBE FLAGS! (#define USE_PROBE_FLAGS)
#define USE_PROBE_FLAGS
#endif
#ifndef USE_TAGGED_FOR_SHORT
#warning THE PROBE FLAGS SHORTCUT REQUIRES TAGGED FLAGS! (#define USE_TAGGED_FLAGS)
#define USE_TAGGED_FLAGS
#endif
#endif

#ifdef USE_TAGGED_FOR_SHORT
#ifndef USE_TAGGED_FLAGS
#warning TAGGED SHORT MESSAGES REQUIRE TAGGED FLAGS! (#define USE_TAGGED_FLAGS)
#define USE_TAGGED_FLAGS
#endif
#endif

#ifdef USE_REMOTE_PUT_LOCAL_GET
#ifndef USE_PROBE_FLAGS
#warning PROBING FOR MESSAGES IN REMOTE-PUT/LOCAL-GET NEEDS ADDITIONAL PROBE FLAGS! (#define USE_PROBE_FLAGS)
#define USE_PROBE_FLAGS
#endif
#endif

#ifdef SCC_COUPLED_SYSTEMS
#ifndef USE_REVERTED_FLAGS
#ifdef USE_TAGGED_FLAGS
#warning COUPLED SYSTEMS REQUIRE REVERTED FLAGS WHEN USING TAGGED FLAGS! (#define USE_REVERTED_FLAGS)
#define USE_REVERTED_FLAGS
#endif
#endif
#ifndef USE_REMOTE_PUT_LOCAL_GET
#warning COUPLED SYSTEMS SHOULD USE REMOTE-PUT/LOCAL-GET! (#define USE_REMOTE_PUT_LOCAL_GET)
#define USE_REMOTE_PUT_LOCAL_GET
#endif
#else
#ifdef USE_PROBE_FLAGS
#warning NON-COUPLED SYSTEMS SHOULD NOT USE ADDITIONAL PROBE FLAGS! (#undef USE_PROBE_FLAGS)
#undef USE_PROBE_FLAGS
#endif
#endif

#ifdef USE_PROBE_FLAGS
#ifdef USE_FAT_BARRIER
#warning PROBABLY TOO LITTLE MPB SPACE FOR USING FAT BARRIER WITH PROBE FLAGS ENABLED! (#undef USE_FAT_BARRIER)
#undef USE_FAT_BARRIER
#endif
#endif

////////////////////////////////////////////////////////////////////////////////////////////////
#else  // !AUTO_ADJUST_SETTINGS

#ifdef SINGLEBITFLAGS
#ifdef USE_TAGGED_FLAGS
#error TAGGED FLAGS CANNOT BE USED WITH SINGLEBITFLAGS! (#undef USE_TAGGED_FLAGS)
#endif
#undef USE_TAGGED_FLAGS
#undef USE_TAGGED_FOR_SHORT
#undef USE_PROBE_FLAGS_SHORTCUT
#ifdef USE_FAT_BARRIER
#error FAT BARRIER CANNOT BE USED WITH SINGLEBITFLAGS! (#undef USE_FAT_BARRIER)
#endif
#endif

#ifdef USE_PROBE_FLAGS_SHORTCUT
#ifndef USE_PROBE_FLAGS
#error THE PROBE FLAGS SHORTCUT REQUIRES PROBE FLAGS! (#define USE_PROBE_FLAGS)
#endif
#ifndef USE_TAGGED_FOR_SHORT
#error THE PROBE FLAGS SHORTCUT REQUIRES TAGGED FLAGS! (#define USE_TAGGED_FLAGS)
#endif
#endif

#ifdef USE_TAGGED_FOR_SHORT
#ifndef USE_TAGGED_FLAGS
#error TAGGED SHORT MESSAGES REQUIRE TAGGED FLAGS! (#define USE_TAGGED_FLAGS)
#endif
#endif

#ifdef USE_REMOTE_PUT_LOCAL_GET
#ifndef USE_PROBE_FLAGS
#warning PROBING FOR MESSAGES IN REMOTE-PUT/LOCAL-GET NEEDS ADDITIONAL PROBE FLAGS! (#define USE_PROBE_FLAGS)
#endif
#endif

#ifdef SCC_COUPLED_SYSTEMS
#ifdef USE_TAGGED_FLAGS
#ifndef USE_REVERTED_FLAGS
#error COUPLED SYSTEMS REQUIRE REVERTED FLAGS WHEN USING TAGGED FLAGS! (#define USE_REVERTED_FLAGS)
#endif
#endif
#ifndef USE_REMOTE_PUT_LOCAL_GET
#warning COUPLED SYSTEMS SHOULD USE REMOTE-PUT/LOCAL-GET! (#define USE_REMOTE_PUT_LOCAL_GET)
#endif
#else
#ifdef USE_PROBE_FLAGS
#warning NON-COUPLED SYSTEMS SHOULD NOT USE ADDITIONAL PROBE FLAGS! (#undef USE_PROBE_FLAGS)
#endif
#endif

#ifdef USE_PROBE_FLAGS
#ifdef USE_FAT_BARRIER
#warning PROBABLY TOO LITTLE MPB SPACE FOR USING FAT BARRIER WITH PROBE FLAGS ENABLED! (#undef USE_FAT_BARRIER)
#endif
#endif


#endif // !AUTO_ADJUST_SETTINGS
////////////////////////////////////////////////////////////////////////////////////////////////


/* PAD32byte is used to compute a cacheline padded length of n (input) bytes */
#define PAD32byte(n) ((n)%32==0 ? (n) : (n) + 32 - (n)%32)

//#define BITSPERCHAR                     8

#define BOTH_IN_COMM_BUFFER             12
#define SOURCE_IN_PRIVATE_MEMORY        34
#define TARGET_IN_PRIVATE_MEMORY        56

#ifdef SINGLEBITFLAGS
#define RCCE_FLAGS_PER_BYTE 8
#else 
#define RCCE_FLAGS_PER_BYTE 1
#endif
#define RCCE_FLAGS_PER_LINE (RCCE_LINE_SIZE*RCCE_FLAGS_PER_BYTE)

#define RCCE_SUM_INT                       (RCCE_SUM+(RCCE_NUM_OPS)*(RCCE_INT))
#define RCCE_SUM_LONG                      (RCCE_SUM+(RCCE_NUM_OPS)*(RCCE_LONG))
#define RCCE_SUM_FLOAT                     (RCCE_SUM+(RCCE_NUM_OPS)*(RCCE_FLOAT))
#define RCCE_SUM_DOUBLE                    (RCCE_SUM+(RCCE_NUM_OPS)*(RCCE_DOUBLE))
#define RCCE_MAX_INT                       (RCCE_MAX+(RCCE_NUM_OPS)*(RCCE_INT))
#define RCCE_MAX_LONG                      (RCCE_MAX+(RCCE_NUM_OPS)*(RCCE_LONG))
#define RCCE_MAX_FLOAT                     (RCCE_MAX+(RCCE_NUM_OPS)*(RCCE_FLOAT))
#define RCCE_MAX_DOUBLE                    (RCCE_MAX+(RCCE_NUM_OPS)*(RCCE_DOUBLE))
#define RCCE_MIN_INT                       (RCCE_MIN+(RCCE_NUM_OPS)*(RCCE_INT))
#define RCCE_MIN_LONG                      (RCCE_MIN+(RCCE_NUM_OPS)*(RCCE_LONG))
#define RCCE_MIN_FLOAT                     (RCCE_MIN+(RCCE_NUM_OPS)*(RCCE_FLOAT))
#define RCCE_MIN_DOUBLE                    (RCCE_MIN+(RCCE_NUM_OPS)*(RCCE_DOUBLE))
#define RCCE_PROD_INT                      (RCCE_PROD+(RCCE_NUM_OPS)*(RCCE_INT))
#define RCCE_PROD_LONG                     (RCCE_PROD+(RCCE_NUM_OPS)*(RCCE_LONG))
#define RCCE_PROD_FLOAT                    (RCCE_PROD+(RCCE_NUM_OPS)*(RCCE_FLOAT))
#define RCCE_PROD_DOUBLE                   (RCCE_PROD+(RCCE_NUM_OPS)*(RCCE_DOUBLE))

#define RCCE_COMM_INITIALIZED              45328976
#define RCCE_COMM_NOT_INITIALIZED          -45328976

// auxiliary MPB pointer type
typedef volatile unsigned int*  t_vintp;
// Also need dereferenced types
typedef volatile unsigned char t_vchar;
typedef volatile unsigned int  t_vint;

typedef struct rcce_block {
  t_vcharp space;          // pointer to space for data in block             
  size_t free_size;        // actual free space in block (0 or whole block)  
  size_t size;             // size of an allocated block
  struct rcce_block *next; // pointer to next block in circular linked list 
} RCCE_BLOCK;

#if defined(SINGLEBITFLAGS) || defined(USE_BYTE_FLAGS)
typedef struct rcce_flag_line {
  char flag[RCCE_FLAGS_PER_LINE];
  t_vcharp line_address;
  int  members;
  struct rcce_flag_line *next;
} RCCE_FLAG_LINE;
#endif


typedef struct  {
  RCCE_BLOCK *tail;     // "last" block in linked list of blocks           
} RCCE_BLOCK_S;

#ifdef AIR
#define FPGA_BASE 0xf9000000
#define BACKOFF_MIN 8
#define BACKOFF_MAX 256
typedef volatile struct _RCCE_AIR {
        int * counter;
        int * init;
} RCCE_AIR;
#endif

#ifndef GORY
  extern RCCE_FLAG    RCCE_sent_flag[RCCE_MAXNP];
  extern RCCE_FLAG    RCCE_ready_flag[RCCE_MAXNP];
#ifdef USE_PIPELINE_FLAGS
  extern RCCE_FLAG    RCCE_sent_flag_pipe[RCCE_MAXNP];
  extern RCCE_FLAG    RCCE_ready_flag_pipe[RCCE_MAXNP];
#endif
#ifdef USE_PROBE_FLAGS
  extern RCCE_FLAG RCCE_probe_flag[RCCE_MAXNP];
#endif
  extern t_vcharp     RCCE_buff_ptr;
  extern size_t       RCCE_chunk;
  extern t_vcharp     RCCE_flags_start; 
#ifndef USE_REMOTE_PUT_LOCAL_GET
  extern RCCE_SEND_REQUEST* RCCE_send_queue;
  extern RCCE_RECV_REQUEST* RCCE_recv_queue[RCCE_MAXNP];
#else
  extern RCCE_SEND_REQUEST* RCCE_send_queue[RCCE_MAXNP];
  extern RCCE_RECV_REQUEST* RCCE_recv_queue;
#endif
#endif

//#ifdef USE_FLAG_EXPERIMENTAL
extern t_vcharp     RCCE_flag_buffer[RCCE_MAXNP];
//#endif

#ifndef __hermit__
extern t_vcharp     RCCE_fool_write_combine_buffer;
#endif
extern t_vcharp     RCCE_comm_buffer[RCCE_MAXNP];
extern int          RCCE_NP;
extern int          RCCE_BUFF_SIZE;
#ifndef COPPERRIDGE
  extern omp_lock_t RCCE_corelock[RCCE_MAXNP];
  extern t_vchar    RC_comm_buffer[RCCE_MAXNP*RCCE_BUFF_SIZE_MAX];
  extern t_vchar    RC_shm_buffer[RCCE_SHM_SIZE_MAX];
#endif
extern int          RC_MY_COREID;
extern int          RC_COREID[RCCE_MAXNP];
extern double       RC_REFCLOCKGHZ;
extern int          RCCE_IAM;
extern int          RCCE_debug_synch;
extern int          RCCE_debug_comm;
extern int          RCCE_debug_debug;
extern int          RCCE_debug_RPC;
#ifdef SINGLEBITFLAGS
  extern RCCE_FLAG_LINE RCCE_flags;
  extern int            WORDSIZE;
  extern int            LEFTMOSTBIT;
  RCCE_FLAG_STATUS RCCE_bit_value(t_vcharp, int);
  RCCE_FLAG_STATUS RCCE_flip_bit_value(t_vcharp, int);
  int RCCE_write_bit_value(t_vcharp, int, RCCE_FLAG_STATUS);
#endif

extern int          RCCE_comm_init_val;

void     RCCE_malloc_init(t_vcharp, size_t);
void     RCCE_shmalloc_init(t_vcharp, size_t);
int      RCCE_qsort(char *, size_t, size_t, int (*)(const void*, const void*));
int      id_compare(const void *, const void *);
#if 0
int      RCCE_probe(RCCE_FLAG);
#endif
int      RCCE_error_return(int, int);
#ifdef __hermit__
#define RC_cache_invalidate()	{}
#else
void     RC_cache_invalidate(void);
#endif
int 	 RCCE_acquire_treelock(RCCE_COMM*);
int 	 RCCE_release_treelock(RCCE_COMM*);
int      RCCE_TNS_barrier(RCCE_COMM*);
int      RCCE_acquire_lock(int);
int	 RCCE_try_lock(int);
int	 RCCE_backoff_lock(int);
int      RCCE_release_lock(int);
int      RCCE_global_color(int, void *);
t_vcharp RC_COMM_BUFFER_START(int);
//#ifdef USE_FLAG_EXPERIMENTAL
t_vcharp RC_FLAG_BUFFER_START(int);
//#endif

#ifndef GORY
  t_vcharp RCCE_malloc(size_t);
  t_vcharp RCCE_malloc_request(size_t, size_t *);
  t_vcharp RCCE_palloc(size_t, int);
  void     RCCE_free(t_vcharp);
  int      RCCE_put(t_vcharp, t_vcharp, int, int);
  int      RCCE_get(t_vcharp, t_vcharp, int, int);
  int      RCCE_wait_until(RCCE_FLAG, RCCE_FLAG_STATUS);
  int      RCCE_test_flag(RCCE_FLAG, RCCE_FLAG_STATUS, int *);
  int      RCCE_flag_alloc(RCCE_FLAG *);
  int      RCCE_flag_free(RCCE_FLAG *);
  int      RCCE_flag_write(RCCE_FLAG *, RCCE_FLAG_STATUS, int); 
  int      RCCE_flag_read(RCCE_FLAG, RCCE_FLAG_STATUS *, int);
#ifdef USE_FLAG_EXPERIMENTAL
  int      RCCE_put_flag(t_vcharp, t_vcharp, int, int);
  int      RCCE_get_flag(t_vcharp, t_vcharp, int, int);
#endif
#ifdef USE_TAGGED_FLAGS
  int      RCCE_flag_write_tagged(RCCE_FLAG *, RCCE_FLAG_STATUS, int, void*, int); 
  int      RCCE_flag_read_tagged(RCCE_FLAG, RCCE_FLAG_STATUS *, int, void*, int);
  int      RCCE_wait_tagged(RCCE_FLAG, RCCE_FLAG_STATUS, void *, int);
  int      RCCE_test_tagged(RCCE_FLAG, RCCE_FLAG_STATUS, int *, void *, int);
#endif
#endif

#if defined(_OPENMP) && !defined(__hermit__)
  #pragma omp threadprivate (RC_COREID, RC_MY_COREID, RC_REFCLOCKGHZ)
  #pragma omp threadprivate (RCCE_comm_buffer)
  #pragma omp threadprivate (RCCE_BUFF_SIZE)
  #pragma omp threadprivate (RCCE_IAM, RCCE_NP)
  #pragma omp threadprivate (RCCE_debug_synch, RCCE_debug_comm, RCCE_debug_debug)
  #ifdef SINGLEBITFLAGS
    #pragma omp threadprivate (RCCE_flags, WORDSIZE, LEFTMOSTBIT)
  #endif
  #ifndef GORY
    #pragma omp threadprivate (RCCE_send_queue, RCCE_recv_queue)
    #pragma omp threadprivate (RCCE_sent_flag, RCCE_ready_flag)
#ifdef USE_PROBE_FLAGS
    #pragma omp threadprivate (RCCE_probe_flag)
#endif
#ifdef USE_PIPELINE_FLAGS
    #pragma omp threadprivate (RCCE_sent_flag_pipe, RCCE_ready_flag_pipe)
#endif
    #pragma omp threadprivate (RCCE_buff_ptr, RCCE_chunk)
    #pragma omp threadprivate (RCCE_flags_start)
  #endif
#endif

#ifdef SHMADD
unsigned int getCOREID();
unsigned int readTILEID();
unsigned int readLUT(unsigned int);
void         writeLUT(unsigned int, unsigned int);
#endif 

#endif
