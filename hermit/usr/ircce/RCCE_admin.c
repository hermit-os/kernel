//***************************************************************************************
// Administrative routines. 
//***************************************************************************************
//
// Author: Rob F. Van der Wijngaart
//         Intel Corporation
// Date:   008/30/2010
//
//***************************************************************************************
//
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
#include "RCCE_lib.h"
#ifdef RC_POWER_MANAGEMENT
  #include "RCCE_lib_pwr.h"
#endif

#ifdef COPPERRIDGE
#ifndef SCC
#define SCC
#endif
#endif

#ifdef SCC
  #include <unistd.h>
  #include <stdlib.h>
  #include <stdint.h>
  #include <limits.h>
#ifndef __hermit__
  #include <sys/mman.h>
  #include "SCC_API.h"
#else
  #define RCCE_SESSION_ID	42
  #include "syscall.h"
  extern unsigned int get_cpufreq();
#endif
#endif
  #include <sys/types.h>
  #include <sys/stat.h>
  #include <fcntl.h>

// En-/ or disable debug prints...
#define DEBUG 1
#define LOCKDEBUG 1

#undef SHMDBG

#ifdef __hermit__
static inline int tas(t_vcharp lock)
{
	register unsigned char _res = 1;

	asm volatile(
                "lock; xchgb %0,%1"
		: "=q"(_res), "=m"(*lock)
		: "0"(_res));
	return (int) _res;
}
#define Test_and_Set(a) tas(virtual_lockaddress[a])
#elif defined(SCC)
// Test and Set method
#define Test_and_Set(a) ((*(virtual_lockaddress[a])) & 0x01)
#endif
#define BACKOFF_MIN 8
#define BACKOFF_MAX 256

#ifdef __hermit__
typedef struct islelock {
	// Internal queue
	int32_t queue;
	// Internal dequeue
	int32_t dequeue;
} islelock_t;

extern islelock_t* rcce_lock;

/*
 *  * Use a own implementation of "atomic_add_return" to gurantee
 *   * that the lock prefix is used.
 *    */
inline static int _hermit_atomic_add(int32_t *d, int i)
{
	int res = i;
	asm volatile("lock; xaddl %0, %1" : "=r"(i) : "m"(*d), "0"(i) : "memory", "cc");
	return res+i;
}

static inline int islelock_lock(void)
{
	int ticket;

	ticket = _hermit_atomic_add(&rcce_lock->queue, 1);
	while(rcce_lock->dequeue != ticket) {
		asm volatile ("pause");
	}

	return 0;
}

static inline int islelock_unlock(void)
{
	_hermit_atomic_add(&rcce_lock->dequeue, 1);

	return 0;
}
#endif

//......................................................................................
// GLOBAL VARIABLES USED BY THE LIBRARY
//......................................................................................
unsigned int next;
int       RCCE_NP;               // number of participating cores
int       RCCE_DEVICE_NR;             // device number of the scc board
int       RCCE_NUM_DEVICES;      // total number of scc boards involved
int       RCCE_NUM_UES_DEVICE[RCCE_MAX_BOARDS]; // number of participating cores per board
int       RCCE_UE_TO_DEVICE[RCCE_MAXNP]; // device id of each core
int       RCCE_DEVICE_LOCAL_UE;  // device-local core id
double    RC_REFCLOCKGHZ;        // baseline CPU frequency (GHz)
int       RC_MY_COREID;          // physical ID of calling core
int       RC_COREID[RCCE_MAXNP]; // array of physical core IDs for all participating 
                                 // cores, sorted by rank
int       RCCE_IAM=-1;           // rank of calling core (invalid by default)
RCCE_COMM RCCE_COMM_WORLD;       // predefined global communicator
int       RCCE_BUFF_SIZE;        // available MPB size
t_vcharp  RCCE_comm_buffer[RCCE_MAXNP]; // starts of MPB, sorted by rank
#ifndef __hermit__
//#ifdef USE_FLAG_EXPERIMENTAL
t_vcharp  RCCE_flag_buffer[RCCE_MAXNP];
//#endif
#endif
#ifndef GORY
  // ......................... non-GORY communication mode .............................
  // synchronization flags are predefined and maintained by the library
  RCCE_FLAG RCCE_sent_flag[RCCE_MAXNP], RCCE_ready_flag[RCCE_MAXNP];
#ifdef USE_PIPELINE_FLAGS
  RCCE_FLAG RCCE_sent_flag_pipe[RCCE_MAXNP], RCCE_ready_flag_pipe[RCCE_MAXNP];
#endif
#ifdef USE_PROBE_FLAGS
  RCCE_FLAG RCCE_probe_flag[RCCE_MAXNP];
#endif
  RCCE_FLAG RCCE_barrier_flag[RCCE_MAXNP];
  RCCE_FLAG RCCE_barrier_release_flag;
  // payload part of the MPBs starts at a specific address, not malloced space
  t_vcharp RCCE_buff_ptr;
  // maximum chunk size of message payload is also specified
  size_t RCCE_chunk;
  // synchronization flags will be allocated at this address
  t_vcharp  RCCE_flags_start;

#ifndef USE_REMOTE_PUT_LOCAL_GET
  // send request queue
  RCCE_SEND_REQUEST* RCCE_send_queue;
  // recv request queue
  RCCE_RECV_REQUEST* RCCE_recv_queue[RCCE_MAXNP];
#else
  // send request queue
  RCCE_SEND_REQUEST* RCCE_send_queue[RCCE_MAXNP];
  // recv request queue
  RCCE_RECV_REQUEST* RCCE_recv_queue;
#endif

#endif // !GORY

#ifndef __hermit__
t_vcharp RCCE_fool_write_combine_buffer;
#endif
// int air_counter = 0;

#ifdef SCC
  // virtual addresses of test&set registers
  t_vcharp virtual_lockaddress[RCCE_MAXNP];
#endif
//......................................................................................
// END GLOBAL VARIABLES USED BY THE LIBRARY
//......................................................................................

#ifdef SCC
#ifdef __hermit__
inline volatile uint64_t _rdtsc() {
	register uint64_t lo, hi;
	asm volatile ("rdtsc" : "=a"(lo), "=d"(hi) );
	return ((uint64_t)hi << 32ULL | (uint64_t)lo);
}
#elif defined(__INTEL_COMPILER)
    inline volatile long long _rdtsc() {
      register long long TSC __asm__("eax");
      __asm__ volatile (".byte 15, 49" : : : "eax", "edx");
      return TSC;
    }
#endif
#endif

//--------------------------------------------------------------------------------------
// FUNCTION: RC_cache_invalidate
//--------------------------------------------------------------------------------------
// invalidate (not flush!) lines in L1 that map to MPB lines
//--------------------------------------------------------------------------------------
#ifndef __hermit__
void RC_cache_invalidate() {
#ifdef SCC
  __asm__ volatile ( ".byte 0x0f; .byte 0x0a;\n" ); // CL1FLUSHMB
#endif
  return;
}
#endif

static inline void RC_wait(int wait) {
#ifdef __hermit__
  asm volatile(	      "movq %%rax, %%rcx\n\t"
		      "L1: nop\n\t"
		      "loop L1"
		      : /* no output registers */
		      : "a" (wait)
		      : "%rcx" );
#else
  asm volatile(       "movl %%eax,%%ecx\n\t"
                      "L1: nop\n\t"
                      "loop L1"
                      : /* no output registers */
                      : "a" (wait)
                      : "%ecx" );
  return;
#endif
}

//--------------------------------------------------------------------------------------
// FUNCTION: RC_COMM_BUFFER_SIZE
//--------------------------------------------------------------------------------------
// return total available MPB size on chip
//--------------------------------------------------------------------------------------
int RC_COMM_BUFFER_SIZE() {
  return RCCE_BUFF_SIZE_MAX*RCCE_MAXNP;
}

//--------------------------------------------------------------------------------------
// FUNCTION: RC_COMM_BUFFER_START
//--------------------------------------------------------------------------------------
// return (virtual) start address of MPB for UE with rank ue
//--------------------------------------------------------------------------------------
t_vcharp RC_COMM_BUFFER_START(int ue){
#ifdef __hermit__
  t_vcharp retval;
  retval =  (t_vcharp) sys_rcce_malloc(RCCE_SESSION_ID, RC_COREID[ue]);
  if (!retval) {
    fprintf(stderr, "rcce_malloc failed\n");
    RCCE_finalize();
    exit(1);
  }
  return retval;
#elif defined(SCC)
  // "Allocate" MPB, using memory mapping of physical addresses
  t_vcharp retval;
#ifndef SCC_COUPLED_SYSTEMS
  MPBalloc(&retval, X_PID(RC_COREID[ue]), Y_PID(RC_COREID[ue]), Z_PID(RC_COREID[ue]), 
           (X_PID(RC_COREID[ue]) == X_PID(RC_COREID[RCCE_IAM])) && 
           (Y_PID(RC_COREID[ue]) == Y_PID(RC_COREID[RCCE_IAM]))
          );
#else
  MPBalloc(&retval, X_PID(RC_COREID[ue]), Y_PID(RC_COREID[ue]), Z_PID(RC_COREID[ue]), RC_COREID[ue] / RCCE_MAXNP_PER_BOARD, RCCE_DEVICE_NR,
	   (X_PID(RC_COREID[ue]) == X_PID(RC_COREID[RCCE_IAM])) && 
           (Y_PID(RC_COREID[ue]) == Y_PID(RC_COREID[RCCE_IAM]))
          );
#endif
  return retval;
#else
  // even in functional emulation mode we leave gaps in the global MPB
  return RC_comm_buffer + RC_COREID[ue]*RC_COMM_BUFFER_SIZE()/RCCE_MAXNP;
#endif
}

#ifndef __hermit__
//#ifdef USE_FLAG_EXPERIMENTAL
t_vcharp RC_FLAG_BUFFER_START(int ue){
  // "Allocate" MPB, using memory mapping of physical addresses
  t_vcharp retval;
#if SCC_COUPLED_SYSTEMS
  FLAGalloc(&retval, X_PID(RC_COREID[ue]), Y_PID(RC_COREID[ue]), Z_PID(RC_COREID[ue]), RC_COREID[ue] / RCCE_MAXNP_PER_BOARD, RCCE_DEVICE_NR, (X_PID(RC_COREID[ue]) == X_PID(RC_COREID[RCCE_IAM])) &&
             (Y_PID(RC_COREID[ue]) == Y_PID(RC_COREID[RCCE_IAM]))
           );
#else
  FLAGalloc(&retval, X_PID(RC_COREID[ue]), Y_PID(RC_COREID[ue]), Z_PID(RC_COREID[ue]),(X_PID(RC_COREID[ue]) == X_PID(RC_COREID[RCCE_IAM])) &&
             (Y_PID(RC_COREID[ue]) == Y_PID(RC_COREID[RCCE_IAM]))
           );
#endif
  return retval;
}
//#endif
#endif

//--------------------------------------------------------------------------------------
// FUNCTION: RC_SHM_BUFFER_START
//--------------------------------------------------------------------------------------
// return (virtual) start address of off-chip shared memory
//--------------------------------------------------------------------------------------
#ifndef __hermit__
#ifndef SCC_COUPLED_SYSTEMS
t_vcharp RC_SHM_BUFFER_START(){
#ifdef SCC
  t_vcharp retval;
  SHMalloc(&retval); //SHMalloc() is in SCC_API.c
  return retval;
#else
  return RC_shm_buffer;
#endif
}
#else
t_vcharp RC_SHM_BUFFER_START(int device){
  t_vcharp retval;
  if (device == RCCE_DEVICE_NR)
    SHMalloc(&retval);
  else
    RMalloc(&retval, device);

  return retval;
}
#endif
#endif

extern int isle_id(void);

//--------------------------------------------------------------------------------------
// FUNCTION: MYCOREID
//--------------------------------------------------------------------------------------
// return physical core ID of calling core
//--------------------------------------------------------------------------------------
int MYCOREID() {
#ifdef __hermit__
  return isle_id();
#elif defined(SCC)
  int tmp, x, y, z;
  tmp=ReadConfigReg(CRB_OWN+MYTILEID);
  x=(tmp>>3) & 0x0f; // bits 06:03
  y=(tmp>>7) & 0x0f; // bits 10:07
  z=(tmp   ) & 0x07; // bits 02:00
#ifndef SCC_COUPLED_SYSTEMS
  return ( ( x + ( 6 * y ) ) * 2 ) + z; // True Processor ID!
#else
   return ( ( x + ( 6 * y ) ) * 2 ) + z + RCCE_MAXNP_PER_BOARD * RCCE_DEVICE_NR; // True Processor ID!
#endif
#else
  // the COREIDs are read into the main program in potentially random order.
  // Each core can access its own Core ID. We simulate that by selecting
  // the value in the list of coreids that corresponds to the sequence
  // number of the OpenMP thread number                                  
  return RC_COREID[omp_get_thread_num()];
#endif // SCC
}

#if defined(SCC)
//--------------------------------------------------------------------------------------
// FUNCTIONS: Locksuite for test-purpose
//--------------------------------------------------------------------------------------
// acquire lock corresponding to core with rank ID
//--------------------------------------------------------------------------------------
int RCCE_try_lock(int ID) {
  if (Test_and_Set(ID))
    return(RCCE_SUCCESS);
  return(RCCE_PENDING);
}

int RCCE_TNS_barrier(RCCE_COMM* comm) {

// two roundtrips to realize a barrier using a T&S Register for each core.

// 1. search first free T&S Register to spin
// 2. last waiter wakes up first waiter and continues local wait
// 3. first waiter wakes up second waiter by releasing its lock ...
// At least every used T&S Register is 0 and no UE can overtake a barrier.

  int num = comm->size;
  int step = 0;
  //fprintf(stderr,"%d:\t enter barrier \n",id);

  while( !Test_and_Set(step) ) ++step;
  // only one UE runs until T&S # num-1

  //fprintf(stderr,"%d:\t step %d\n",id,step);

  if(step == num-1) {
    //fprintf(stderr,"%d:\t I am the last one\n",id);
    *(virtual_lockaddress[0]) = 0x0;
    while(!Test_and_Set(step));
    *(virtual_lockaddress[step]) = 0x0;
  } else {
    while(!Test_and_Set(step));
    *(virtual_lockaddress[step]) = 0x0;
    *(virtual_lockaddress[step+1]) = 0x0;
  }
  //fprintf(stderr,"released barrier! step: %d\n", step);
  return RCCE_SUCCESS;
}

int RCCE_nb_TNS_barrier(RCCE_COMM* comm) {

// two roundtrips to realize a barrier using a T&S Register for each core.

// 1. search first free T&S Register to spin
// 2. last waiter wakes up first waiter and continues local wait
// 3. first waiter wakes up second waiter by releasing its lock ...
// At least every used T&S Register is 0 and no UE can overtake a barrier.

  int num = comm->size;
  int step = 0;
  //fprintf(stderr,"%d:\t enter barrier \n",id);

  if(comm->label == 1) goto label1;
  if(comm->label == 2) goto label2;

  while( !Test_and_Set(step) ) ++step;
  // only one UE runs until T&S # num-1

  //fprintf(stderr,"%d:\t step %d\n",id,step);

  if(step == num-1) {
    //fprintf(stderr,"%d:\t I am the last one\n",id);
    *(virtual_lockaddress[0]) = 0x0;
    comm->step = step;
  label1:
    step = comm->step;
    if(!Test_and_Set(step))
    {
      comm->label = 1;
      return RCCE_PENDING;
    }
    *(virtual_lockaddress[step]) = 0x0;
  } else {
    comm->step = step;
  label2:
    step = comm->step;
    if(!Test_and_Set(step))
    {
      comm->label = 2;
      return RCCE_PENDING;
    }
    *(virtual_lockaddress[step]) = 0x0;
    *(virtual_lockaddress[step+1]) = 0x0;
  }
  //fprintf(stderr,"released barrier! step: %d\n", step);
  comm->label = 0;
  return RCCE_SUCCESS;
}

#ifdef AIR
RCCE_AIR RCCE_atomic_inc_regs[2*RCCE_MAXNP];

int RCCE_AIR_barrier2(RCCE_COMM *comm)
{
  static int idx = 0;
  unsigned long long time, time1, time2;
  float ran = 0;
  int id, val = 0, val2 = 0;
  int window = comm->size;
  int ue = RCCE_ue();
  int x = X_PID(ue), y = Y_PID(ue);
  int win = 1000000;

  // ++air_counter;
  if (comm == &RCCE_COMM_WORLD) {
    time = RCCE_wtime();
    if ((id = *RCCE_atomic_inc_regs[idx].counter) < (comm->size-1)) 
    {
      if(window > 16) {
        val = id;
        val2 = val;
        time1 = RCCE_wtime();;

        if(window > 26)
        {
          ran = ((y+x)%8)*window*window/24000000.0;
          window = (RCCE_wtime() - time)*win;//(RCCE_wtime() - time)*1000000.0;
        }
        else
          window = 1;
        ran = ran+(rand()%(window))/(win*100.0);
        do
        {
          time = RCCE_wtime() - time;
          time2 = RCCE_wtime()-time1-time/2;
          time1 = RCCE_wtime();
          while(RCCE_wtime()-time1 < (((0.424+ran)*(comm->size-val)*(time2)/(val-val2+1)-time/2)))
          {
            if(RCCE_wtime()-time1>0.0050)
              break;
          }
          val2 = val;
          time = RCCE_wtime();
          // ++air_counter;
        } while ((val = *RCCE_atomic_inc_regs[idx].init) > 0 && (val < comm->size));
      }
      else
      {
        do
        {
          // ++air_counter;
        }
        while ((val = *RCCE_atomic_inc_regs[idx].init) > 0 && (val < comm->size));
      }
      
    }
    else
    {
      *RCCE_atomic_inc_regs[idx].init = 0;	
    }
    idx = !idx;
    return(RCCE_SUCCESS);
  }
  else
  {
    return RCCE_barrier(comm);
  }
}

#ifndef GORY
int RCCE_dissemination_barrier(RCCE_COMM *comm)
{
  int k, max_rounds;
  int ue, num_ues, ue_signal;
  ue = RCCE_ue();
  num_ues = RCCE_num_ues();
  max_rounds = num_ues*(1+(num_ues%2)?1:0);

  for(k = 1; k < max_rounds; k = k*2 )
  {
    /* signalize process */
    ue_signal = (ue+k)%num_ues;
    RCCE_flag_write(&RCCE_barrier_flag[RCCE_IAM], RCCE_FLAG_SET, ue_signal);
    /* wait for process */
    ue_signal = (ue-k+num_ues+num_ues)%num_ues;
    RCCE_wait_until(RCCE_barrier_flag[ue_signal], RCCE_FLAG_SET);
    RCCE_flag_write(&RCCE_barrier_flag[ue_signal], RCCE_FLAG_UNSET, RCCE_IAM);
  }

  return(RCCE_SUCCESS);
}
#endif

int RCCE_tree_init(RCCE_COMM *comm, tree_t *tree, int num_children) {
  int ue, num_ues;
  int i, j, k;
  tree_t nodes[RCCE_MAXNP];
  if(comm != &RCCE_COMM_WORLD)
    return(!RCCE_SUCCESS);
  ue = RCCE_ue();
  num_ues = RCCE_num_ues();

  nodes[0].parent = -1;
  k = 1;

  for(i = 0; i < num_ues; ++i)
  {
    nodes[i].num_children = 0;
    for(j = 0; j < num_children && k < num_ues; ++j, ++k)
    {
      nodes[i].child[j] = k;
      nodes[k].parent = i;
      ++(nodes[i].num_children);
    }
  }
  memcpy(tree, &nodes[RCCE_IAM], sizeof(tree_t));

  // printf("%d: child0:%d child1:%d parent:%d\n", ue, tree->child[0], tree->child[1], tree->parent);fflush(0);

  return(RCCE_SUCCESS);
}

#ifndef GORY
int RCCE_tree_barrier(RCCE_COMM *comm, tree_t *tree)
{
  int i;
  /* Gather */
  for(i = 0; i < tree->num_children; ++i)
  {
    RCCE_wait_until(RCCE_barrier_flag[tree->child[i]], RCCE_FLAG_SET);
    RCCE_flag_write(&RCCE_barrier_flag[tree->child[i]], RCCE_FLAG_UNSET, RCCE_IAM);
  }

  if(tree->parent != -1)
  {
    RCCE_flag_write(&RCCE_barrier_flag[RCCE_IAM], RCCE_FLAG_SET, tree->parent);

    /* Release */
    RCCE_wait_until(RCCE_barrier_release_flag, RCCE_FLAG_SET);
    RCCE_flag_write(&RCCE_barrier_release_flag, RCCE_FLAG_UNSET, RCCE_IAM);
  }

  /* Release */
  for(i = 0; i < tree->num_children; ++i)
  {
    RCCE_flag_write(&RCCE_barrier_release_flag, RCCE_FLAG_SET, tree->child[i]);
  }
  
  return(RCCE_SUCCESS);
}
#endif

int RCCE_tournament_barrier(RCCE_COMM *comm)
{
  return(RCCE_SUCCESS);
}

int RCCE_tournament_fixed_barrier(RCCE_COMM *comm)
{
  return(RCCE_SUCCESS);
}

int RCCE_AIR_barrier(RCCE_COMM *comm)
{
  static int idx = 0;
  static unsigned int rand = 0;
  int backoff = BACKOFF_MIN, wait, i = 0;

  if (comm == &RCCE_COMM_WORLD) {
    if (*RCCE_atomic_inc_regs[idx].counter < (comm->size-1)) 
    {
      while (*RCCE_atomic_inc_regs[idx].init > 0)
      {
        rand = rand * 1103515245u + 12345u;
        wait = BACKOFF_MIN + (rand % (backoff << i));
        RC_wait(wait);
        if (wait < BACKOFF_MAX) i++;
      }
    }
    else
    {
      *RCCE_atomic_inc_regs[idx].init = 0;	
    }
    idx = !idx;
    return(RCCE_SUCCESS);
  }
  else
  {
    return RCCE_barrier(comm);
  }
}

int RCCE_nb_AIR_barrier(RCCE_COMM *comm)
{
  static int idx = 0;
  static unsigned int rand = 0;
  int backoff = BACKOFF_MIN, wait, i = 0;

  if(comm->label == 1) goto label1;

  if (comm == &RCCE_COMM_WORLD) {
    if (*RCCE_atomic_inc_regs[idx].counter < (comm->size-1)) 
    {
#if 0 // NO BACKOFF in Non-Blocking case ???
      while (*RCCE_atomic_inc_regs[idx].init > 0)
      {
        rand = rand * 1103515245u + 12345u;
        wait = BACKOFF_MIN + (rand % (backoff << i));
        RC_wait(wait);
        if (wait < BACKOFF_MAX) i++;
      }
#else
    label1:
      if(*RCCE_atomic_inc_regs[idx].init > 0)
      {
	comm->label = 1;
	return RCCE_PENDING;
      }
#endif
    }
    else
    {
      *RCCE_atomic_inc_regs[idx].init = 0;	
    }
    idx = !idx;
    comm->label = 0;
    return(RCCE_SUCCESS);
  }
  else
  {
    return RCCE_barrier(comm);
  }
}
#endif

int RCCE_acquire_treelock(RCCE_COMM* comm) {
  int i = 1; // concurrency factor
  int step;
  int group = (1 << i);
  int me = comm->my_rank;

  //fprintf(stdout,"%d\tstart treelock:\n", me);
  while (1){

    //group <<= 1;
    //if(group > num) break;

    // first rank within group + mid of group (leftmost)
    step = ( me - ( me % group) ) + ( ( group - 1 ) >> 1 ) ;

    //fprintf(stdout,"%d\t%d\n", me, step);
    //fflush(stdout);
    while(!Test_and_Set(comm->member[step]));
    
    if(group >= comm->size) break;
    
    group <<= i;
  }// while ( group <= comm->size);
  // group is next 2^x

  //fprintf(stdout,"\n");
  //fflush(stderr);
  return(RCCE_SUCCESS);
}

int RCCE_release_treelock(RCCE_COMM* comm) {//int myID, int num) { 
  int step;
  int group;
  int v = comm->size;
  int me = comm->my_rank;

  // round up to the next highest power of 2
  v--;
  v |= v >> 1;
  v |= v >> 2;
  v |= v >> 4;
  v |= v >> 8;
  v |= v >> 16;
  v++;
  // 
  group = v;

  //printf(stderr,"%d\trelease treelock: [%d] ",myID,group);

  while(1) {
    step = ( me - ( me % group) ) + ( ( group - 1 ) >> 1 );
    //fprintf(stderr," %d",step);
    *(virtual_lockaddress[(comm->member[step])]) = 0x0;
    group >>= 1;
    if(group < 2) break;
  }
  //fprintf(stderr,"\n");
  //fflush(stderr);
  return(RCCE_SUCCESS);
}

int RCCE_backoff_lock(int ID) {
  //static int next = RC_MY_COREID;
  // try lock with backoff
 
  int i = 0;
 
  int backoff = BACKOFF_MIN, wait = 0, tmp = 0;
  unsigned int overflow = 0;
  

  while (1) {
    if (Test_and_Set(ID))
      break;

    // Kongruenzgenerator
    next = ( next * 1103515245 + 12345 ) % ( INT_MAX );

    wait = BACKOFF_MIN + ( next % ( backoff << i ) );

    overflow += wait;
    if( overflow > INT_MAX ) overflow = INT_MAX;

    RC_wait(wait);
    if ( (backoff<<i) < BACKOFF_MAX) i++;
  }

  tmp = (int)overflow;

# if (LOCKDEBUG)
    return tmp;
# endif
  return(RCCE_SUCCESS);
}
#endif

//--------------------------------------------------------------------------------------
// FUNCTION: RCCE_acquire_lock
//--------------------------------------------------------------------------------------
// acquire lock corresponding to core with rank ID
//--------------------------------------------------------------------------------------
int RCCE_acquire_lock(int ID) {

#ifdef __hermit__
  islelock_lock();
#elif defined(SCC)
  // semantics of test&set register: a read returns zero if another core has
  // previously read it and no reset has occurred since then. Otherwise, the read
  // returns one. Comparing (hex) one with the contents of the register forces a
  // read. As long as the comparison fails, we keep reading.
# if (LOCKDEBUG)
      int tmp = 0;
      while (!Test_and_Set(ID)) ++tmp;
      return tmp;
# else
      while (!Test_and_Set(ID)) ;
# endif
#else
  omp_set_lock(&(RCCE_corelock[ID]));
#endif
  return(RCCE_SUCCESS);
}

//--------------------------------------------------------------------------------------
// FUNCTION: RCCE_release_lock
//--------------------------------------------------------------------------------------
// release lock corresponding to core with rank ID
//--------------------------------------------------------------------------------------
int RCCE_release_lock(int ID) {
#ifdef __hermit__
  islelock_unlock();
#elif defined(SCC)
  // semantics of test&set register: a write by _any_ core causes a reset
  *(virtual_lockaddress[ID]) = 0x0;
#else
  omp_unset_lock(&(RCCE_corelock[ID]));
#endif
  return RCCE_SUCCESS;
}

//--------------------------------------------------------------------------------------
// FUNCTION: RC_FREQUENCY
//--------------------------------------------------------------------------------------
// return actual core clock frequency (Hz)
//--------------------------------------------------------------------------------------
long long RC_FREQUENCY() {
return (long long)(RC_REFCLOCKGHZ*1.e9);
}

//--------------------------------------------------------------------------------------
// FUNCTION: RCCE_init
//--------------------------------------------------------------------------------------
// initialize the library and sanitize parameter list
//--------------------------------------------------------------------------------------
int RCCE_init(
  int *argc,   // pointer to argc, passed in from main program
  char ***argv // pointer to argv, passed in from main program
  ) {
  int ue;
#ifdef SCC
  #ifdef SCC_COUPLED_SYSTEMS
    int board;
  #endif
  #ifndef __hermit__
    int x, y, z;
    unsigned int physical_lockaddress;
  #endif
#endif
#ifdef SHMADD
  int i;
  unsigned int RCCE_SHM_BUFFER_offset ,result, rd_slot_nbr, wr_slot_nbr;
#endif
  void *nothing = NULL;

  int verbose_level = 0;

#ifdef __hermit__
  sys_rcce_init(RCCE_SESSION_ID /* id of the session */);
#elif defined(SCC)
  // Copperridge specific initialization...
  InitAPI(0);fflush(0);
#endif

  // save pointer to executable name for later insertion into the argument list
  char *executable_name = (*argv)[0];

  if(getenv("MPID_SCC_VERBOSITY_LEVEL") != NULL)
  {
    verbose_level = atoi(getenv("MPID_SCC_VERBOSITY_LEVEL"));
  }

#ifdef __hermit__
  RCCE_DEVICE_NR      = 0;
#elif defined(SCC) && defined(SCC_COUPLED_SYSTEMS)
  RCCE_DEVICE_NR      = atoi(*(++(*argv)));  
#else
  RCCE_DEVICE_NR      = 0;
#endif

  RCCE_NP        = atoi(*(++(*argv)));  
#ifdef __hermit__
  // HermitCore ignores the third argument and uses
  // its own clock value
  RC_REFCLOCKGHZ = (double) get_cpufreq() / 1000.0;
  ++(*argv);
#else
  RC_REFCLOCKGHZ = atof(*(++(*argv)));
#endif

  // put the participating core ids (unsorted) into an array             
  for (ue=0; ue<RCCE_NP; ue++) {
    RC_COREID[ue] = atoi(*(++(*argv)));
  }

#ifndef SCC
  // if using the functional emulator, must make sure to have read all command line 
  // parameters up to now before overwriting (shifted) first one with executable
  // name; even though argv is made firstprivate, that applies only the pointer to 
  // the arguments, not the actual data
  #pragma omp barrier
#endif
  // make sure executable name is as expected                 
  (*argv)[0] = executable_name;

  RC_MY_COREID = MYCOREID();

  next = RC_MY_COREID;

  // adjust apparent number of command line arguments, so it will appear to main 
  // program that number of UEs, clock frequency, and core ID list were not on
  // command line        
#ifndef SCC_COUPLED_SYSTEMS
  *argc -= RCCE_NP + 2;
#else
  *argc -= RCCE_NP + 3;
#endif

  if(RCCE_NP == 1) {
    RCCE_IAM = 0;
  }
  else {

    // sort array of participating phyical core IDs to determine their ranks
    RCCE_qsort((char *)RC_COREID, RCCE_NP, sizeof(int), id_compare);
    
    // determine rank of calling core
    for (ue=0; ue<RCCE_NP; ue++) {
      if (RC_COREID[ue] == RC_MY_COREID) RCCE_IAM = ue;
    }
  }

#ifdef SHMADD
//   printf("Using SHMADD\n");
     RCCE_SHM_BUFFER_offset     = 0x00;
//   RCCE_SHM_BUFFER_offset     = 0x3FFFF80;
//   RCCE_SHM_BUFFER_offset   = 0x4000000;
//   RCCE_SHM_BUFFER_offset   = 0x181000;
   rd_slot_nbr=0x80;
   for(i=0; i<60; i++) {
     result  = readLUT(rd_slot_nbr);
     result -= 1;
     wr_slot_nbr = rd_slot_nbr + 4;
     writeLUT(wr_slot_nbr,result);
     rd_slot_nbr++;
   }
#endif

  // leave in one reassuring debug print
  if (DEBUG) {
    printf("My rank is %d, physical core ID is %d\n", RCCE_IAM, RC_MY_COREID);
    fflush(0);
  }

  if (RCCE_IAM<0)
    return(RCCE_error_return(RCCE_debug_comm,RCCE_ERROR_CORE_NOT_IN_HOSTFILE));

#if defined(SCC)
  // compute and memory map addresses of test&set registers for all participating cores 
  for (ue=0; ue<RCCE_NP; ue++) { 
#ifdef __hermit__
    virtual_lockaddress[ue] = (t_vcharp) ((size_t)rcce_lock + (ue+1) * RCCE_LINE_SIZE);
#else
    z = Z_PID(RC_COREID[ue]);
    x = X_PID(RC_COREID[ue]);
    y = Y_PID(RC_COREID[ue]);
#ifndef SCC_COUPLED_SYSTEMS
    physical_lockaddress = CRB_ADDR(x,y) + (z==0 ? LOCK0 : LOCK1);
#else
    physical_lockaddress = CRB_ADDR(x, y, RC_COREID[ue] / RCCE_MAXNP_PER_BOARD, RCCE_DEVICE_NR) + (z==0 ? LOCK0 : LOCK1);
#endif
    virtual_lockaddress[ue] = (t_vcharp) MallocConfigReg(physical_lockaddress);
#endif
  }
#endif

  // initialize MPB starting addresses for all participating cores; allow one
  // dummy cache line at front of MPB for fooling write combine buffer in case
  // of single-byte MPB access
#ifndef __hermit__
  RCCE_fool_write_combine_buffer = RC_COMM_BUFFER_START(RCCE_IAM);
#endif

  for (ue=0; ue<RCCE_NP; ue++) 
    RCCE_comm_buffer[ue] = RC_COMM_BUFFER_START(ue) + RCCE_LINE_SIZE;

  // gross MPB size is set equal to maximum
  RCCE_BUFF_SIZE = RCCE_BUFF_SIZE_MAX - RCCE_LINE_SIZE;

#ifndef __hermit__
//#ifdef USE_FLAG_EXPERIMENTAL
  for (ue=0; ue<RCCE_NP; ue++) {
    RCCE_flag_buffer[ue] = RC_FLAG_BUFFER_START(ue) + RCCE_LINE_SIZE;
  }
//#endif
#endif

#ifdef RC_POWER_MANAGEMENT
#ifndef SCC
  // always store RPC queue data structure at beginning of MPB, so allocatable
  // storage needs to skip it. Only need to do this for functional emulator
  for (ue=0; ue<RCCE_NP; ue++) {
//#ifdef USE_FLAG_EXPERIMENTAL
    RCCE_flag_buffer[ue] += REGULATOR_LENGTH;
//#endif
    RCCE_comm_buffer[ue] += REGULATOR_LENGTH;
  }
  RCCE_BUFF_SIZE -= REGULATOR_LENGTH;
#endif
#endif

  // initialize RCCE_malloc
  RCCE_malloc_init(RCCE_comm_buffer[RCCE_IAM],RCCE_BUFF_SIZE);

#ifndef __hermit__
#ifdef SHMADD

  RCCE_shmalloc_init(RC_SHM_BUFFER_START()+RCCE_SHM_BUFFER_offset ,RCCE_SHM_SIZE_MAX);
#ifdef SHMDBG
  printf("\n%d:%s:%d: RCCE_SHM_BUFFER_offset, RCCE_SHM_SIZE_MAX: % x %x\n", RCCE_IAM, 
    __FILE__,__LINE__,RCCE_SHM_BUFFER_offset ,RCCE_SHM_SIZE_MAX);
#endif
#else

#ifndef SCC_COUPLED_SYSTEMS
  RCCE_shmalloc_init(RC_SHM_BUFFER_START(),RCCE_SHM_SIZE_MAX);
#else
  for(board=RCCE_MAX_BOARDS-1; board>=0; board--)
    RCCE_shmalloc_init(RC_SHM_BUFFER_START(board),RCCE_SHM_SIZE_MAX/RCCE_MAX_BOARDS);
#endif
#endif
#endif

  // create global communicator (equivalent of MPI_COMM_WORLD); this will also allocate 
  // the two synchronization flags associated with the global barrier 
  RCCE_comm_split(RCCE_global_color, nothing, &RCCE_COMM_WORLD);

  // if power management is enabled, initialize more stuff; this includes two more 
  // communicators (for voltage and frequency domains), plus two synchronization flags
  // associated with the barrier for each communicator       
#ifdef RC_POWER_MANAGEMENT
  int error;
  if (error=RCCE_init_RPC(RC_COREID, RCCE_IAM, RCCE_NP)) 
       return(RCCE_error_return(RCCE_debug_RPC,error));
#endif

#ifndef GORY
  // if we use the simplified API, we need to define more flags upfront  
  for (ue=0; ue<RCCE_NP; ue++) {
    RCCE_flag_alloc(&RCCE_sent_flag[ue]);
    RCCE_flag_alloc(&RCCE_ready_flag[ue]);
#ifdef USE_PIPELINE_FLAGS
    RCCE_flag_alloc(&RCCE_sent_flag_pipe[ue]);
    RCCE_flag_alloc(&RCCE_ready_flag_pipe[ue]);
#endif
#ifdef USE_PROBE_FLAGS
    RCCE_flag_alloc(&RCCE_probe_flag[ue]);
#endif
    RCCE_flag_alloc(&RCCE_barrier_flag[ue]);
  }  
    RCCE_flag_alloc(&RCCE_barrier_release_flag);

#ifndef USE_REMOTE_PUT_LOCAL_GET
  RCCE_send_queue = NULL;
  for (ue=0; ue<RCCE_NP; ue++) {
    RCCE_recv_queue[ue] = NULL;
  }
#else
  RCCE_recv_queue = NULL;
  for (ue=0; ue<RCCE_NP; ue++) {
    RCCE_send_queue[ue] = NULL;
  }
#endif

#endif

#if defined(SCC) && defined(SCC_COUPLED_SYSTEMS)
  int tmp, dev;
  if(RCCE_NP > 1) {
    if(RCCE_IAM != RCCE_NP-1) {
      RCCE_send((char*)&RCCE_DEVICE_NR, sizeof(int), RCCE_IAM+1);  
    }
    if(RCCE_IAM != 0) {
      RCCE_recv((char*)&tmp, sizeof(int), RCCE_IAM-1);
      if(tmp != RCCE_DEVICE_NR) tmp = RCCE_IAM;
      else tmp = -1;
      RCCE_send((char*)&tmp, sizeof(int), 0);
    }
    else
    {
      RCCE_NUM_DEVICES = 0;
      for(ue=1; ue<RCCE_NP; ue++) {
	RCCE_recv((char*)&tmp, sizeof(int), ue);
	if(tmp != -1) {
	  if(RCCE_NUM_DEVICES == 0)
	    RCCE_NUM_UES_DEVICE[0] = tmp;	  
	  else
	    RCCE_NUM_UES_DEVICE[RCCE_NUM_DEVICES] = tmp - RCCE_NUM_UES_DEVICE[RCCE_NUM_DEVICES-1];
	  RCCE_NUM_DEVICES++;
	}
      }
      RCCE_NUM_DEVICES++;
      for(dev=0, tmp=0; dev<RCCE_NUM_DEVICES; dev++)
	tmp += RCCE_NUM_UES_DEVICE[dev];
      RCCE_NUM_UES_DEVICE[RCCE_NUM_DEVICES-1] = RCCE_NP - tmp;
    }
    RCCE_bcast((char*)&RCCE_NUM_DEVICES, sizeof(int), 0, RCCE_COMM_WORLD);
    RCCE_bcast((char*)&RCCE_NUM_UES_DEVICE, RCCE_MAX_BOARDS * sizeof(int), 0, RCCE_COMM_WORLD);

    for(ue=0; ue<RCCE_NP; ue++) {
      for(dev=0, tmp=0; dev<RCCE_NUM_DEVICES; dev++)
      {
	if(ue == RCCE_IAM) RCCE_DEVICE_LOCAL_UE = RCCE_IAM - tmp;
	tmp += RCCE_NUM_UES_DEVICE[dev];
	if(ue < tmp){
	  RCCE_UE_TO_DEVICE[ue] = dev;
	  //printf("(%d) RCCE_UE_TO_DEVICE[%d] = %d\n", RCCE_IAM, ue, dev);	  
	  break;
	}
      }
    }
    //printf("(%d) RCCE_DEVICE_LOCAL_UE = %d\n", RCCE_IAM, RCCE_DEVICE_LOCAL_UE);
  }
  else
#endif
  {
    RCCE_NUM_DEVICES = 1;
    RCCE_NUM_UES_DEVICE[0] = RCCE_NP;
    RCCE_DEVICE_LOCAL_UE = RCCE_IAM;    
    for(ue=0; ue<RCCE_NP; ue++) RCCE_UE_TO_DEVICE[ue] = 0;
  }

#ifdef AIR
  {
    int * air_base = (int *) MallocConfigReg(FPGA_BASE + 0xE000);

    // Assign and Initialize First Set of Atomic Increment Registers
    for (i = 0; i < RCCE_MAXNP; i++)
    {
      RCCE_atomic_inc_regs[i].counter = air_base + 2*i;
      RCCE_atomic_inc_regs[i].init = air_base + 2*i + 1;
      if(RCCE_IAM == 0)
	*RCCE_atomic_inc_regs[i].init = 0;
    }
    // Assign and Initialize Second Set of Atomic Increment Registers
    air_base = (int *) MallocConfigReg(FPGA_BASE + 0xF000);
    for (i = 0; i < RCCE_MAXNP; i++) 
    {
      RCCE_atomic_inc_regs[RCCE_MAXNP+i].counter = air_base + 2*i;
      RCCE_atomic_inc_regs[RCCE_MAXNP+i].init = air_base + 2*i + 1;
      if(RCCE_IAM == 0)
	*RCCE_atomic_inc_regs[RCCE_MAXNP+i].init = 0;
    }
  }
#endif

#ifndef GORY
  if( (RCCE_IAM == 0) && (verbose_level > 1) )
  {
    printf("### %s: Remaining MPB space for communication: %zd Bytes per core\n", executable_name, RCCE_chunk); fflush(stdout);
  }
#endif

  RCCE_barrier(&RCCE_COMM_WORLD);

  return (RCCE_SUCCESS);
}

//--------------------------------------------------------------------------------------
// FUNCTION:  RCCE_finalize
//--------------------------------------------------------------------------------------
// clean up at end of library usage (memory unmapping) and resetting of memory and
// registers
//--------------------------------------------------------------------------------------
int RCCE_finalize(void){

#ifdef SCC
#ifndef __hermit__
  int ue, iword;
#endif

  RCCE_barrier(&RCCE_COMM_WORLD);

  // each UE clears its own MPB and test&set register
  //ERROR: THIS IS NOT THE START OF THE COMM BUFFER, BUT OF THE PAYLOAD AREA!!
//  for (iword=0; iword<(RCCE_BUFF_SIZE_MAX)/sizeof(int); iword++)
//      ((int *)(RCCE_comm_buffer[ue]))[iword] = 0;
//    MPBunalloc(&(RCCE_comm_buffer[ue]));
#ifndef __hermit__
  RCCE_release_lock(RCCE_IAM);
  // each core needs to unmap all special memory locations
  for (ue=0; ue<RCCE_NP; ue++) { 
    FreeConfigReg((int *)(virtual_lockaddress[ue]));
  }
#else
  sys_rcce_fini(RCCE_SESSION_ID /* id of the session */);
#endif
  fflush(NULL);
#endif
  return (RCCE_SUCCESS);
}

//--------------------------------------------------------------------------------------
// FUNCTION:  RCCE_wtime
//--------------------------------------------------------------------------------------
// clean up at end of library usage (memory unmapping)
//--------------------------------------------------------------------------------------
double RCCE_wtime(void) {
#ifdef SCC
  return ( ((double)_rdtsc())/(RC_REFCLOCKGHZ*1.e9));
#else
  return (omp_get_wtime());
#endif
}

//--------------------------------------------------------------------------------------
// FUNCTION:  RCCE_ue
//--------------------------------------------------------------------------------------
// return rank of calling core
//--------------------------------------------------------------------------------------
int RCCE_ue(void) {return(RCCE_IAM);}

//--------------------------------------------------------------------------------------
// FUNCTION: RCCE_num_ues
//--------------------------------------------------------------------------------------
// return total number of participating UEs              
//--------------------------------------------------------------------------------------
int RCCE_num_ues(void) {return(RCCE_NP);}

#ifdef SCC_COUPLED_SYSTEMS
//--------------------------------------------------------------------------------------
// FUNCTIONS:  RCCE_dev, RCCE_num_devs, RCCE_num_ues_dev
//--------------------------------------------------------------------------------------
// returning ID of own device, total number of devices and number of UEs per device
//--------------------------------------------------------------------------------------
int RCCE_dev(void) {return(RCCE_DEVICE_NR);}
int RCCE_num_dev(void) {return(RCCE_NUM_DEVICES);}
int RCCE_num_ues_dev(int ue) {return(RCCE_NUM_UES_DEVICE[ue]);}
int RCCE_ue_to_dev(int ue) { return(RCCE_UE_TO_DEVICE[ue]);}
int RCCE_dev_ue(void) { return(RCCE_DEVICE_LOCAL_UE);}
#endif

#ifdef SHMADD
//--------------------------------------------------------------------------------------
// FUNCTION: writeLUT
//--------------------------------------------------------------------------------------
void writeLUT(unsigned int lutSlot, unsigned int value) {

int PAGE_SIZE, NCMDeviceFD;
// NCMDeviceFD is the file descriptor for non-cacheable memory (e.g. config regs).

unsigned int result;

t_vcharp     MappedAddr;
unsigned int myCoreID, alignedAddr, pageOffset, ConfigAddr;

   myCoreID = getCOREID();
   if(myCoreID==1)
      ConfigAddr = CRB_OWN+LUT1 + (lutSlot*0x08);
   else
      ConfigAddr = CRB_OWN+LUT0 + (lutSlot*0x08);

   PAGE_SIZE  = getpagesize();

   if ((NCMDeviceFD=open("/dev/rckncm", O_RDWR|O_SYNC))<0) {
    perror("open"); exit(-1);
   }

   alignedAddr = ConfigAddr & (~(PAGE_SIZE-1));
   pageOffset  = ConfigAddr - alignedAddr;

   MappedAddr = (t_vcharp) mmap(NULL, PAGE_SIZE, PROT_WRITE|PROT_READ,
       MAP_SHARED, NCMDeviceFD, alignedAddr);

   if (MappedAddr == MAP_FAILED) {
      perror("mmap");exit(-1);
   }

   *(int*)(MappedAddr+pageOffset) = value;
   munmap((void*)MappedAddr, PAGE_SIZE);

}

//--------------------------------------------------------------------------------------
// FUNCTION: readLUT
//--------------------------------------------------------------------------------------
unsigned int readLUT(unsigned int lutSlot) {

int PAGE_SIZE, NCMDeviceFD;
// NCMDeviceFD is the file descriptor for non-cacheable memory (e.g. config regs).

unsigned int result;
t_vcharp     MappedAddr;
unsigned int myCoreID, alignedAddr, pageOffset, ConfigAddr;

   myCoreID = getCOREID();
   if(myCoreID==1)
      ConfigAddr = CRB_OWN+LUT1 + (lutSlot*0x08);
   else
      ConfigAddr = CRB_OWN+LUT0 + (lutSlot*0x08);

   PAGE_SIZE  = getpagesize();

   if ((NCMDeviceFD=open("/dev/rckncm", O_RDWR|O_SYNC))<0) {
    perror("open"); exit(-1);
   }

   alignedAddr = ConfigAddr & (~(PAGE_SIZE-1));
   pageOffset  = ConfigAddr - alignedAddr;

   MappedAddr = (t_vcharp) mmap(NULL, PAGE_SIZE, PROT_WRITE|PROT_READ,
      MAP_SHARED, NCMDeviceFD, alignedAddr);

   if (MappedAddr == MAP_FAILED) {
      perror("mmap");exit(-1);
   }

  result = *(unsigned int*)(MappedAddr+pageOffset);
  munmap((void*)MappedAddr, PAGE_SIZE);

  return result;
}


//--------------------------------------------------------------------------------------
// FUNCTION: getCOREID
//--------------------------------------------------------------------------------------
unsigned int getCOREID() {

int PAGE_SIZE, NCMDeviceFD;
// NCMDeviceFD is the file descriptor for non-cacheable memory (e.g. config regs).

t_vcharp     MappedAddr;
unsigned int coreID,result,  alignedAddr, pageOffset, ConfigAddr, coreID_mask=0x00000007;


   ConfigAddr = CRB_OWN+MYTILEID;
   PAGE_SIZE  = getpagesize();

   if ((NCMDeviceFD=open("/dev/rckncm", O_RDWR|O_SYNC))<0) {
    perror("open"); exit(-1);
   }

   alignedAddr = ConfigAddr & (~(PAGE_SIZE-1));
   pageOffset  = ConfigAddr - alignedAddr;

   MappedAddr = (t_vcharp) mmap(NULL, PAGE_SIZE, PROT_WRITE|PROT_READ,
      MAP_SHARED, NCMDeviceFD, alignedAddr);

   if (MappedAddr == MAP_FAILED) {
      perror("mmap");exit(-1);
   }

  result = *(unsigned int*)(MappedAddr+pageOffset);
  munmap((void*)MappedAddr, PAGE_SIZE);

  coreID =  result & coreID_mask;
  return coreID;
}
#endif
