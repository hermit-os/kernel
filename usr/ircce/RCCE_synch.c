///*************************************************************************************
// Synchronization functions. 
// Single-bit and whole-cache-line flags are sufficiently different that we provide
// separate implementations of the synchronization routines for each case
//**************************************************************************************
//
// Author: Rob F. Van der Wijngaart
//         Intel Corporation
// Date:   008/30/2010
//
//**************************************************************************************
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
#ifdef __hermit__
#include "rte_memcpy.h"
#define memcpy_scc rte_memcpy
#elif defined(COPPERRIDGE) 
#include "scc_memcpy.h"
#else
#define memcpy_scc memcpy
#endif

#ifdef USE_BYTE_FLAGS
#include "RCCE_byte_synch.c"
#else

#ifdef SINGLEBITFLAGS

//////////////////////////////////////////////////////////////////
// LOCKING SYNCHRONIZATION USING ONE BIT PER FLAG 
//////////////////////////////////////////////////////////////////


//--------------------------------------------------------------------------------------
// FUNCTION: RCCE_wait_until
//--------------------------------------------------------------------------------------
// wait until flag in local MPB becomes set or unset. To avoid reading stale data from 
// the cache instead of new flag value from the MPB, issue MPB cache invalidation before 
// each read, including within the spin cycle 
//--------------------------------------------------------------------------------------
int RCCE_wait_until(RCCE_FLAG flag, RCCE_FLAG_STATUS val) {
  t_vcharp cflag;

  cflag = flag.line_address;

// avoid tests if we use the simplified API 
#ifdef GORY
  if (val != RCCE_FLAG_UNSET && val != RCCE_FLAG_SET) 
    return(RCCE_error_return(RCCE_debug_synch,RCCE_ERROR_FLAG_STATUS_UNDEFINED));
  if (!cflag)
    return(RCCE_error_return(RCCE_debug_synch,RCCE_ERROR_FLAG_NOT_ALLOCATED));
  // check to see if flag is properly contained in the local comm buffer  
  if (cflag - RCCE_comm_buffer[RCCE_IAM]>=0 &&
      cflag+RCCE_LINE_SIZE - (RCCE_comm_buffer[RCCE_IAM] + RCCE_BUFF_SIZE)<0){}
  else {
    return(RCCE_error_return(RCCE_debug_synch,RCCE_ERROR_FLAG_NOT_IN_COMM_BUFFER));
  }
#endif

  // always flush/invalidate to ensure we read the most recent value of *flag
  // keep reading it until it has the required value 
  do {
#ifdef _OPENMP
    #pragma omp flush  
#endif
    RC_cache_invalidate();
  } 
  while ((RCCE_bit_value(cflag, flag.location) != val));

  return(RCCE_SUCCESS);
}

int RCCE_test_flag(RCCE_FLAG flag, RCCE_FLAG_STATUS val, int *result) {
 t_vcharp cflag;

  cflag = flag.line_address;

// avoid tests if we use the simplified API 
#ifdef GORY
  if (val != RCCE_FLAG_UNSET && val != RCCE_FLAG_SET) 
    return(RCCE_error_return(RCCE_debug_synch,RCCE_ERROR_FLAG_STATUS_UNDEFINED));
  if (!cflag)
    return(RCCE_error_return(RCCE_debug_synch,RCCE_ERROR_FLAG_NOT_ALLOCATED));
  // check to see if flag is properly contained in the local comm buffer  
  if (cflag - RCCE_comm_buffer[RCCE_IAM]>=0 &&
      cflag+RCCE_LINE_SIZE - (RCCE_comm_buffer[RCCE_IAM] + RCCE_BUFF_SIZE)<0){}
  else {
    return(RCCE_error_return(RCCE_debug_synch,RCCE_ERROR_FLAG_NOT_IN_COMM_BUFFER));
  }
#endif

  // always flush/invalidate to ensure we read the most recent value of *flag
  // keep reading it until it has the required value 

#ifdef _OPENMP
  #pragma omp flush  
#endif
  RC_cache_invalidate();
   
  if(RCCE_bit_value(cflag, flag.location) != val) {
    (*result) = 0;
  }    
  else {
    (*result) = 1;
  }
    
  return(RCCE_SUCCESS);
} 

//--------------------------------------------------------------------------------------
// FUNCTION: RCCE_barrier
//--------------------------------------------------------------------------------------
// very simple, linear barrier 
//--------------------------------------------------------------------------------------
int RCCE_barrier(RCCE_COMM *comm) {
 
  t_vchar           cyclechar[RCCE_LINE_SIZE] __attribute__ ((aligned (RCCE_LINE_SIZE)));
  t_vchar           valchar  [RCCE_LINE_SIZE] __attribute__ ((aligned (RCCE_LINE_SIZE)));
  int               counter, i, error;
  int               ROOT =  0;
  t_vcharp gatherp, releasep;
  RCCE_FLAG_STATUS  cycle;

  counter = 0;
  gatherp = comm->gather.line_address;
  if (RCCE_debug_synch) 
    fprintf(STDERR,"UE %d has checked into barrier\n", RCCE_IAM);
  // flip local barrier variable
  if (error = RCCE_get(cyclechar, gatherp, RCCE_LINE_SIZE, RCCE_IAM))
    return(RCCE_error_return(RCCE_debug_synch,error));
  cycle = RCCE_flip_bit_value(cyclechar, comm->gather.location);
  if (error = RCCE_put(comm->gather.line_address, cyclechar, RCCE_LINE_SIZE, RCCE_IAM))
    return(RCCE_error_return(RCCE_debug_synch,error));

  if (RCCE_IAM==comm->member[ROOT]) {
    // read "remote" gather flags; once all equal "cycle" (i.e counter==comm->size), 
    // we know all UEs have reached the barrier                   
    while (counter != comm->size) {
      // skip the first member (#0), because that is the ROOT         
      for (counter=i=1; i<comm->size; i++) {
        // copy flag values out of comm buffer                        
        if (error = RCCE_get(valchar, comm->gather.line_address, RCCE_LINE_SIZE, 
                             comm->member[i]))
          return(RCCE_error_return(RCCE_debug_synch,error));
        if (RCCE_bit_value(valchar, comm->gather.location) == cycle) counter++;
      }
    }
    // set release flags                                              
    for (i=1; i<comm->size; i++) 
      if (error = RCCE_flag_write(&(comm->release), cycle, comm->member[i]))
        return(RCCE_error_return(RCCE_debug_synch,error));
  }
  else {
    if (error = RCCE_wait_until(comm->release, cycle))
      return(RCCE_error_return(RCCE_debug_synch,error));
  }
  if (RCCE_debug_synch) fprintf(STDERR,"UE %d has cleared barrier\n", RCCE_IAM);  
  return(RCCE_SUCCESS);
}

#else

//////////////////////////////////////////////////////////////////
// LOCKLESS SYNCHRONIZATION USING ONE WHOLE CACHE LINE PER FLAG //
//////////////////////////////////////////////////////////////////

//--------------------------------------------------------------------------------------
// FUNCTION: RCCE_wait_until
//--------------------------------------------------------------------------------------
// wait until flag in local MPB becomes set or unset. To avoid reading stale data from 
// the cache instead of new flag value from the MPB, issue MPB cache invalidation before 
// each read, including within the spin cycle 
//--------------------------------------------------------------------------------------
int RCCE_wait_until(RCCE_FLAG flag, RCCE_FLAG_STATUS val) {
  t_vcharp cflag;

  cflag = (t_vcharp) flag;
#ifdef GORY
  if (val != RCCE_FLAG_UNSET && val != RCCE_FLAG_SET) 
    return(RCCE_error_return(RCCE_debug_synch,RCCE_ERROR_FLAG_STATUS_UNDEFINED));
  if (!cflag)
    return(RCCE_error_return(RCCE_debug_synch,RCCE_ERROR_FLAG_NOT_ALLOCATED));
  // check to see if flag is properly contained in the local comm buffer  
  if (cflag - RCCE_comm_buffer[RCCE_IAM]>=0 &&
      cflag+RCCE_LINE_SIZE - (RCCE_comm_buffer[RCCE_IAM] + RCCE_BUFF_SIZE)<0){}
  else {
    return(RCCE_error_return(RCCE_debug_synch,RCCE_ERROR_FLAG_NOT_IN_COMM_BUFFER));
  }
#endif

#ifdef USE_REVERTED_FLAGS
  flag = flag + RCCE_LINE_SIZE / sizeof(int) - 1;
#endif

  // always flush/invalidate to ensure we read the most recent value of *flag
  // keep reading it until it has the required value. We only need to read the
  // first int of the MPB cache line containing the flag
#ifndef USE_FLAG_EXPERIMENTAL
  do {
#ifdef _OPENMP
    #pragma omp flush   
#endif
    RC_cache_invalidate();
  } while ((*flag) != val);
#else
  if (RCCE_debug_synch)
    fprintf(STDERR,"UE %d wait flag: %x from address %X \n", RCCE_IAM,val,flag);
  flag = RCCE_flag_buffer[RCCE_IAM]+(flag-RCCE_comm_buffer[RCCE_IAM]);
  while ((*flag) != val);
#endif
  return(RCCE_SUCCESS);
}

#ifdef USE_TAGGED_FLAGS
int RCCE_wait_tagged(RCCE_FLAG flag, RCCE_FLAG_STATUS val, void *tag, int len) {

  int i, j;
  RCCE_FLAG flag_pos;

#ifndef USE_REVERTED_FLAGS
  flag_pos = flag;
#else
  flag_pos = flag + RCCE_LINE_SIZE / sizeof(int) - 1;
#endif

  do {
#ifdef _OPENMP
#pragma omp flush   
#endif
    RC_cache_invalidate();
  } while ((*flag_pos) != val);

  if(tag) {
    if( len > ( RCCE_LINE_SIZE - sizeof(int) ) ) len = RCCE_LINE_SIZE - sizeof(int);
#ifndef USE_REVERTED_FLAGS
    memcpy_scc(tag, &((char*)flag)[sizeof(int)], len);
#else
    memcpy_scc(tag, &((char*)flag)[0], len);
#endif
  }

  return(RCCE_SUCCESS);
}
#endif

int RCCE_test_flag(RCCE_FLAG flag, RCCE_FLAG_STATUS val, int *result) {
  t_vcharp cflag;

  cflag = (t_vcharp) flag;
#ifdef GORY
  if (val != RCCE_FLAG_UNSET && val != RCCE_FLAG_SET) 
    return(RCCE_error_return(RCCE_debug_synch,RCCE_ERROR_FLAG_STATUS_UNDEFINED));
  if (!cflag)
    return(RCCE_error_return(RCCE_debug_synch,RCCE_ERROR_FLAG_NOT_ALLOCATED));
  // check to see if flag is properly contained in the local comm buffer  
  if (cflag - RCCE_comm_buffer[RCCE_IAM]>=0 &&
      cflag+RCCE_LINE_SIZE - (RCCE_comm_buffer[RCCE_IAM] + RCCE_BUFF_SIZE)<0){}
  else {
    return(RCCE_error_return(RCCE_debug_synch,RCCE_ERROR_FLAG_NOT_IN_COMM_BUFFER));
  }
#endif

#ifdef USE_REVERTED_FLAGS
  flag = flag + RCCE_LINE_SIZE / sizeof(int) - 1;
#endif

  // always flush/invalidate to ensure we read the most recent value of *flag
  // keep reading it until it has the required value. We only need to read the
  // first int of the MPB cache line containing the flag
#ifdef _OPENMP
#pragma omp flush   
#endif
#ifndef USE_FLAG_EXPERIMENTAL
  RC_cache_invalidate();
#endif
  if((*flag) != val) {
    (*result) = 0;
  }    
  else {
    (*result) = 1;
  }

  return(RCCE_SUCCESS);
}

#ifdef USE_TAGGED_FLAGS
int RCCE_test_tagged(RCCE_FLAG flag, RCCE_FLAG_STATUS val, int *result, void *tag, int len) {

  int i, j;
  RCCE_FLAG flag_pos;

#ifndef USE_REVERTED_FLAGS
  flag_pos = flag;
#else
  flag_pos = flag + RCCE_LINE_SIZE / sizeof(int) -1;
#endif

  RC_cache_invalidate();

  if((*flag_pos) != val) {
    (*result) = 0;
  }    
  else {
    (*result) = 1;
  }

  if((*result) && tag) {
    if( len > ( RCCE_LINE_SIZE - sizeof(int) ) ) len = RCCE_LINE_SIZE - sizeof(int);
#ifndef USE_REVERTED_FLAGS
    memcpy_scc(tag, &((char*)flag)[sizeof(int)], len);
#else
    memcpy_scc(tag, &((char*)flag)[0], len);
#endif
  }

  return(RCCE_SUCCESS);
}
#endif


//--------------------------------------------------------------------------------------
// FUNCTION: RCCE_barrier
//--------------------------------------------------------------------------------------
// very simple, linear barrier 
//--------------------------------------------------------------------------------------
int RCCE_barrier(RCCE_COMM *comm) {
 
  volatile unsigned char cyclechar[RCCE_LINE_SIZE] __attribute__ ((aligned (RCCE_LINE_SIZE)));
  volatile unsigned char   valchar[RCCE_LINE_SIZE] __attribute__ ((aligned (RCCE_LINE_SIZE)));
  volatile char *cycle;
  volatile char *val;
  int   counter, i, error;
  int   ROOT      =  0;

  counter = 0;
  cycle  = (volatile char *)cyclechar;
  val    = (volatile char *)valchar;

  if (RCCE_debug_synch) 
    fprintf(STDERR,"UE %d has checked into barrier\n", RCCE_IAM);

#ifdef USE_FAT_BARRIER

  // flip local barrier variable
#ifndef USE_FLAG_EXPERIMENTAL
  if ((error = RCCE_get(cyclechar, (t_vcharp)(comm->gather[RCCE_IAM]), RCCE_LINE_SIZE, RCCE_IAM)))
#else
  if ((error = RCCE_get_flag(cyclechar, (t_vcharp)(comm->gather[RCCE_IAM]), RCCE_LINE_SIZE, RCCE_IAM)))
#endif
    return(RCCE_error_return(RCCE_debug_synch,error));
  *cycle = !(*cycle);
#ifndef USE_FLAG_EXPERIMENTAL
  if ((error = RCCE_put((t_vcharp)(comm->gather[RCCE_IAM]), cyclechar, RCCE_LINE_SIZE, RCCE_IAM)))
#else
  if ((error = RCCE_put_flag((t_vcharp)(comm->gather[RCCE_IAM]), cyclechar, RCCE_LINE_SIZE, RCCE_IAM)))
#endif
    return(RCCE_error_return(RCCE_debug_synch,error));
  if ((error = RCCE_put((t_vcharp)(comm->gather[RCCE_IAM]), cyclechar, RCCE_LINE_SIZE, comm->member[ROOT])))
    return(RCCE_error_return(RCCE_debug_synch,error));
 
  if (RCCE_IAM==comm->member[ROOT]) {
    // read "remote" gather flags; once all equal "cycle" (i.e counter==comm->size),
    // we know all UEs have reached the barrier
    while (counter != comm->size) {
      // skip the first member (#0), because that is the ROOT
      for (counter=i=1; i<comm->size; i++) {
	/* copy flag values out of comm buffer */
#ifndef USE_FLAG_EXPERIMENTAL
	if ((error = RCCE_get(valchar, (t_vcharp)(comm->gather[i]), RCCE_LINE_SIZE, RCCE_IAM)))
#else
        if ((error = RCCE_get_flag(valchar, (t_vcharp)(comm->gather[i]), RCCE_LINE_SIZE, RCCE_IAM)))
#endif
	  return(RCCE_error_return(RCCE_debug_synch,error));
	if (*val == *cycle) counter++;
      }
    }
    // set release flags
    for (i=1; i<comm->size; i++) {
      if ((error = RCCE_flag_write(&(comm->release), *cycle, comm->member[i])))
	return(RCCE_error_return(RCCE_debug_synch,error));
    }
  }
  else {
    if ((error = RCCE_wait_until(comm->release, *cycle)))
      return(RCCE_error_return(RCCE_debug_synch,error));
  }

#else // !USE_FAT_BARRIER

  // flip local barrier variable                                      
#ifndef USE_FLAG_EXPERIMENTAL
  if ((error = RCCE_get(cyclechar, (t_vcharp)(comm->gather), RCCE_LINE_SIZE, RCCE_IAM)))
#else
  if ((error = RCCE_get_flag(cyclechar, (t_vcharp)(comm->gather), RCCE_LINE_SIZE, RCCE_IAM)))
#endif
    return(RCCE_error_return(RCCE_debug_synch,error));
  *cycle = !(*cycle);
#ifndef USE_FLAG_EXPERIMENTAL
  if ((error = RCCE_put((t_vcharp)(comm->gather), cyclechar, RCCE_LINE_SIZE, RCCE_IAM)))
#else
  if ((error = RCCE_put_flag((t_vcharp)(comm->gather), cyclechar, RCCE_LINE_SIZE, RCCE_IAM)))
#endif
    return(RCCE_error_return(RCCE_debug_synch,error));

  if (RCCE_IAM==comm->member[ROOT]) {
    // read "remote" gather flags; once all equal "cycle" (i.e counter==comm->size), 
    // we know all UEs have reached the barrier                                            
    while (counter != comm->size) {
      // skip the first member (#0), because that is the ROOT         
      for (counter=i=1; i<comm->size; i++) {
        /* copy flag values out of comm buffer                        */
#ifndef USE_FLAG_EXPERIMENTAL
        if ((error = RCCE_get(valchar, (t_vcharp)(comm->gather), RCCE_LINE_SIZE, 
                             comm->member[i])))
#else
         if ((error = RCCE_get_flag(valchar, (t_vcharp)(comm->gather), RCCE_LINE_SIZE, 
                             comm->member[i])))
#endif
          return(RCCE_error_return(RCCE_debug_synch,error));
        if (*val == *cycle) counter++;
      }
    }
    // set release flags                                             
    for (i=1; i<comm->size; i++) {
      if ((error = RCCE_flag_write(&(comm->release), *cycle, comm->member[i])))
        return(RCCE_error_return(RCCE_debug_synch,error));
    }
  }
  else {
    if ((error = RCCE_wait_until(comm->release, *cycle))) {
      return(RCCE_error_return(RCCE_debug_synch,error));
    }
  }

#endif // !USE_FAT_BARRIER
  if (RCCE_debug_synch) fprintf(STDERR,"UE %d has cleared barrier\n", RCCE_IAM);  
  return(RCCE_SUCCESS);
}

//--------------------------------------------------------------------------------------
// FUNCTION: RCCE_nb_barrier
//--------------------------------------------------------------------------------------
// non-blocking version of the linear barrier 
//--------------------------------------------------------------------------------------
int RCCE_nb_barrier(RCCE_COMM *comm) {
 
  volatile unsigned char cyclechar[RCCE_LINE_SIZE] __attribute__ ((aligned (RCCE_LINE_SIZE)));
  volatile unsigned char   valchar[RCCE_LINE_SIZE] __attribute__ ((aligned (RCCE_LINE_SIZE)));
  int   i, error;
  int   ROOT      =  0;
#ifdef USE_FLAG_EXPERIMENTAL
  volatile char *cycle;
  volatile char *val;
  cycle  = (volatile char *)cyclechar;
  val    = (volatile char *)valchar;
#else
  volatile int *cycle;
  volatile int *val;
  cycle  = (volatile int *)cyclechar;
  val    = (volatile int *)valchar;
#endif

  if(comm->label == 1) goto label1;
  if(comm->label == 2) goto label2;

  comm->count = 0;

  if (RCCE_debug_synch) 
    fprintf(STDERR,"UE %d has checked into barrier\n", RCCE_IAM);

#ifdef USE_FAT_BARRIER

  // flip local barrier variable
#ifndef USE_FLAG_EXPERIMENTAL
  if ((error = RCCE_get(cyclechar, (t_vcharp)(comm->gather[RCCE_IAM]), RCCE_LINE_SIZE, RCCE_IAM)))
#else
  if ((error = RCCE_get_flag(cyclechar, (t_vcharp)(comm->gather[RCCE_IAM]), RCCE_LINE_SIZE, RCCE_IAM)))
#endif
    return(RCCE_error_return(RCCE_debug_synch,error));
  *cycle = !(*cycle);
#ifndef USE_FLAG_EXPERIMENTAL
  if ((error = RCCE_put((t_vcharp)(comm->gather[RCCE_IAM]), cyclechar, RCCE_LINE_SIZE, RCCE_IAM)))
#else
  if ((error = RCCE_put_flag((t_vcharp)(comm->gather[RCCE_IAM]), cyclechar, RCCE_LINE_SIZE, RCCE_IAM)))
#endif
    return(RCCE_error_return(RCCE_debug_synch,error));
  if ((error = RCCE_put((t_vcharp)(comm->gather[RCCE_IAM]), cyclechar, RCCE_LINE_SIZE, comm->member[ROOT])))
    return(RCCE_error_return(RCCE_debug_synch,error));
 
  if (RCCE_IAM==comm->member[ROOT]) {
    // read "remote" gather flags; once all equal "cycle" (i.e counter==comm->size),
    // we know all UEs have reached the barrier
    comm->cycle = *cycle;
label1:
    while (comm->count != comm->size) {
      // skip the first member (#0), because that is the ROOT
      for (comm->count=i=1; i<comm->size; i++) {
	/* copy flag values out of comm buffer */
#ifndef USE_FLAG_EXPERIMENTAL
	if ((error = RCCE_get(valchar, (t_vcharp)(comm->gather[i]), RCCE_LINE_SIZE, RCCE_IAM)))
#else
        if ((error = RCCE_get_flag(valchar, (t_vcharp)(comm->gather[i]), RCCE_LINE_SIZE, RCCE_IAM)))
#endif
	  return(RCCE_error_return(RCCE_debug_synch,error));
	if (*val == comm->cycle) comm->count++;
      }
      if(comm->count != comm->size) {
	comm->label = 1;
	return(RCCE_PENDING);
      }
    }
    // set release flags
    for (i=1; i<comm->size; i++) {
      if ((error = RCCE_flag_write(&(comm->release), comm->cycle, comm->member[i])))
	return(RCCE_error_return(RCCE_debug_synch,error));
    }   
  }
  else {
    int test;
    comm->cycle = *cycle;
label2:
    RCCE_test_flag(comm->release, comm->cycle, &test);
    if(!test) {
      comm->label = 2;
      return(RCCE_PENDING);
    }
  }

  comm->label = 0;

#else // !USE_FAT_BARRIER

  // flip local barrier variable
#ifndef USE_FLAG_EXPERIMENTAL
  if ((error = RCCE_get(cyclechar, (t_vcharp)(comm->gather[0]), RCCE_LINE_SIZE, RCCE_IAM)))
#else
  if ((error = RCCE_get_flag(cyclechar, (t_vcharp)(comm->gather[0]), RCCE_LINE_SIZE, RCCE_IAM)))
#endif
    return(RCCE_error_return(RCCE_debug_synch,error));
  *cycle = !(*cycle);
#ifndef USE_FLAG_EXPERIMENTAL
  if ((error = RCCE_put((t_vcharp)(comm->gather[0]), cyclechar, RCCE_LINE_SIZE, RCCE_IAM)))
#else
  if ((error = RCCE_put_flag((t_vcharp)(comm->gather[0]), cyclechar, RCCE_LINE_SIZE, RCCE_IAM)))
#endif
    return(RCCE_error_return(RCCE_debug_synch,error));

  if (RCCE_IAM==comm->member[ROOT]) {
    // read "remote" gather flags; once all equal "cycle" (i.e counter==comm->size), 
    // we know all UEs have reached the barrier
    comm->cycle = *cycle;
label1:    
    while (comm->count != comm->size) {
      // skip the first member (#0), because that is the ROOT         
      for (comm->count=i=1; i<comm->size; i++) {
        /* copy flag values out of comm buffer                        */
#ifndef USE_FLAG_EXPERIMENTAL
        if ((error = RCCE_get(valchar, (t_vcharp)(comm->gather[0]), RCCE_LINE_SIZE, 
                             comm->member[i])))
#else
         if ((error = RCCE_get_flag(valchar, (t_vcharp)(comm->gather[0]), RCCE_LINE_SIZE, 
                             comm->member[i])))
#endif
          return(RCCE_error_return(RCCE_debug_synch,error));
        if (*val == comm->cycle) comm->count++;
      }
      if(comm->count != comm->size) {
	comm->label = 1;
	return(RCCE_PENDING);
      }
    }
    // set release flags                                              
    for (i=1; i<comm->size; i++) {
      if ((error = RCCE_flag_write(&(comm->release), comm->cycle, comm->member[i])))
        return(RCCE_error_return(RCCE_debug_synch,error));
    }
  }
  else {
    int test;
    comm->cycle = *cycle;
label2:
    RCCE_test_flag(comm->release, comm->cycle, &test);
    if(!test) {
      comm->label = 2;
      return(RCCE_PENDING);
    }
  }

  comm->label = 0;

#endif // !USE_FAT_BARRIER
  if (RCCE_debug_synch) fprintf(STDERR,"UE %d has cleared barrier\n", RCCE_IAM);  
  return(RCCE_SUCCESS);
}

#endif

void RCCE_fence() {
  return;
}

#endif
