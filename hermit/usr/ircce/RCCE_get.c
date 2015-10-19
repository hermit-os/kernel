//***************************************************************************************
// Get data from communication buffer. 
//***************************************************************************************
//
// Author: Rob F. Van der Wijngaart
//         Intel Corporation
// Date:   008/30/2010
//
//***************************************************************************************
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
//    [2010-11-03] switched to SCC-optimized memcpy() functions in scc_memcpy.h:
//                 - memcpy_to_mpb()
//                 - memcpy_from_mpb() 
//                 by Stefan Lankes, Carsten Clauss, Chair for Operating Systems,
//                                                   RWTH Aachen University
//
#include "RCCE_lib.h"

#ifdef __hermit__
#include "rte_memcpy.h"
#define memcpy_from_mpb rte_memcpy
#elif defined(COPPERRIDGE)
#include "scc_memcpy.h"
#else
#define memcpy_form_mpb memcpy
#endif

void *RCCE_memcpy_get(void *dest, const void *src, size_t count)
{ // function wrapper for external usage of improved memcpy()...
#ifdef COPPERRIDGE
  return memcpy_from_mpb(dest, src, count);
#else
  return memcpy(dest, src, count);
#endif
}

#ifdef COPPERRIDGE
#define RCCE_memcpy_get(a,b,c) memcpy_from_mpb(a,b,c)
#else
#define RCCE_memcpy_get(a,b,c) memcpy(a,b,c)
#endif

//--------------------------------------------------------------------------------------
// FUNCTION: RCCE_get
//--------------------------------------------------------------------------------------
// copy data from address "source" in the remote MPB to address "target" in either the
// local MPB, or in the calling UE's private memory. We do not test to see if a move
// into the calling UE's private memory stays within allocated memory                     *
//--------------------------------------------------------------------------------------
int RCCE_get(
  t_vcharp target, // target buffer, MPB or private memory
  t_vcharp source, // source buffer, MPB
  int num_bytes,   // number of bytes to copy (must be multiple of cache line size
  int ID           // rank of source UE
  ) {

//  printf("UE %d at top of RCCE_get\n", RCCE_IAM); fflush(NULL);

#ifdef GORY
  // we only need to do tests in GORY mode; in non-GORY mode ths function is never 
  // called by the user, but only be the library
  int copy_mode;

  // check validity of parameters                                        
  if (!target) return(RCCE_error_return(RCCE_debug_comm,RCCE_ERROR_TARGET));
  if (!source) return(RCCE_error_return(RCCE_debug_comm,RCCE_ERROR_SOURCE));

  if (ID<0 || ID>=RCCE_NP) return(RCCE_error_return(RCCE_debug_comm,RCCE_ERROR_ID));

  if (num_bytes <0 || num_bytes%RCCE_LINE_SIZE!=0) 
      return(RCCE_error_return(RCCE_debug_comm,RCCE_ERROR_MESSAGE_LENGTH));

  // determine if source data is in MPB; check using local buffer boundaries 
  if (source - RCCE_comm_buffer[RCCE_IAM] >=0 &&
      source+num_bytes - (RCCE_comm_buffer[RCCE_IAM] + RCCE_BUFF_SIZE)<=0)
    // shift source address to point to remote MPB                
    source = RCCE_comm_buffer[ID]+(source-RCCE_comm_buffer[RCCE_IAM]);
  else  return(RCCE_error_return(RCCE_debug_comm,RCCE_ERROR_SOURCE));

  // target can be either local MPB or private memory             
  if (target -RCCE_comm_buffer[RCCE_IAM] >= 0 &&
      target+num_bytes - (RCCE_comm_buffer[RCCE_IAM] + RCCE_BUFF_SIZE)<=0)
    copy_mode = BOTH_IN_COMM_BUFFER;
  else 
    copy_mode = TARGET_IN_PRIVATE_MEMORY;

  // make sure that if the copy is between locations within the same MPB
  // there is no overlap between source and target address ranges  
  if ( copy_mode == BOTH_IN_COMM_BUFFER) {
    if (((source-target)>0 && (source+num_bytes-target)<0) ||
        ((target-source)>0 && (target+num_bytes-source)<0)) {
      return(RCCE_error_return(RCCE_debug_comm,RCCE_ERROR_DATA_OVERLAP));
    }
  }

  // ascertain that the start of the buffer is  cache line aligned  
  int start_index = source-RCCE_comm_buffer[ID];
  if (start_index%RCCE_LINE_SIZE!=0) 
      return(RCCE_error_return(RCCE_debug_comm,RCCE_ERROR_ALIGNMENT));

  // only verify alignment of the target if it is in the MPB 
  if (copy_mode == BOTH_IN_COMM_BUFFER) {
    start_index = target-RCCE_comm_buffer[ID];
    if (start_index%RCCE_LINE_SIZE!=0) 
      return(RCCE_error_return(RCCE_debug_comm,RCCE_ERROR_ALIGNMENT));
  }
#else
    // in non-GORY mode we only need to retain the MPB source shift; we
    // already know the source is in the MPB, not private memory
    source = RCCE_comm_buffer[ID]+(source-RCCE_comm_buffer[RCCE_IAM]);
#endif

//  printf("UE %d; target = %x, source = %x, nbytes= %d\n", RCCE_IAM, target, source, num_bytes); 
  fflush(NULL);

  // do the actual copy, making sure we copy fresh data                  
#ifdef _OPENMP
  #pragma omp flush
#endif
  RC_cache_invalidate();

  RCCE_memcpy_get((void *)target, (void *)source, num_bytes);

  if (RCCE_debug_synch)
    fprintf(STDERR,"UE %d get data: %d from address %p \n", RCCE_IAM,*target,source);

//  printf("UE %d finished the memcopy\n", RCCE_IAM);

  // flush data to make sure it is visible to all threads; cannot use a flush list 
  // because it concerns malloced space                     
#ifdef _OPENMP
  #pragma omp flush
#endif
  return(RCCE_SUCCESS);
}

#ifdef USE_FLAG_EXPERIMENTAL
int RCCE_get_flag(
  t_vcharp target, // target buffer, private memory
  t_vcharp source, // source buffer, MPB ncm mapped
  int num_bytes,   // number of bytes to copy (must be multiple of cache line size
  int ID           // rank of source UE
  ) {

  source = RCCE_flag_buffer[ID]+(source-RCCE_comm_buffer[RCCE_IAM]);

  //memcpy((void*)target, (void*)source, num_bytes);

  *target = *source;

  if (RCCE_debug_synch)
    fprintf(STDERR,"UE %d get flag: %x from address %X \n", RCCE_IAM,*target,source);

  return(RCCE_SUCCESS);
}
#endif
