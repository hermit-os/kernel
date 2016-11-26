//***************************************************************************************
// Put data into communication buffer. 
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
#elif defined(COPPERRIDGE)
#include "scc_memcpy.h"
#endif

void *RCCE_memcpy_put(void *dest, const void *src, size_t count)
{ // function wrapper for external usage of improved memcpy()...
#ifdef __hermit__
  return rte_memcpy(dest, src, count);
#elif defined(COPPERRIDGE)
  return memcpy_to_mpb(dest, src, count);
#else
  return memcpy(dest, src, count);
#endif
}

#if defined(COPPERRIDGE) && !defined(__hermit__)
#define RCCE_memcpy_put(a,b,c) memcpy_to_mpb(a, b, c)
#else
#define RCCE_memcpy_put(a,b,c) memcpy(a, b, c)
#endif

//--------------------------------------------------------------------------------------
// FUNCTION: RCCE_put
//--------------------------------------------------------------------------------------
// copy data from address "source" in the local MPB or the calling UE's private memory 
// to address "target" in the remote MPB. We do not test to see if a move from the 
// calling UE's private memory stays within allocated memory                        
//--------------------------------------------------------------------------------------
int RCCE_put(
  t_vcharp target, // target buffer, MPB
  t_vcharp source, // source buffer, MPB or private memory
  int num_bytes, 
  int ID
  ) {

#ifdef GORY
  // we only need to do tests in GORY mode; in non-GORY mode ths function is never 
  // called by the user, but only be the library
  int copy_mode;

  // check validity of parameters                                        
  if (!target) return(RCCE_error_return(RCCE_debug_comm,RCCE_ERROR_TARGET));
  if (!source) return(RCCE_error_return(RCCE_debug_comm,RCCE_ERROR_SOURCE));
  if (ID<0 || 
      ID>=RCCE_NP) return(RCCE_error_return(RCCE_debug_comm,RCCE_ERROR_ID));
  if (num_bytes < 0 || num_bytes%RCCE_LINE_SIZE!=0) 
     return(RCCE_error_return(RCCE_debug_comm,RCCE_ERROR_MESSAGE_LENGTH));
  // determine if target data is in MPB; check using local buffer boundaries 
  if (target - RCCE_comm_buffer[RCCE_IAM]>=0 &&
      target+num_bytes - (RCCE_comm_buffer[RCCE_IAM] + RCCE_BUFF_SIZE)<=0)
    // shift target address to point to remote MPB                
    target = RCCE_comm_buffer[ID]+(target-RCCE_comm_buffer[RCCE_IAM]);    
  else  return(RCCE_error_return(RCCE_debug_comm,RCCE_ERROR_TARGET));

   // source can be either local MPB or private memory            
  if (source - RCCE_comm_buffer[RCCE_IAM] >= 0 &&
      source+num_bytes - (RCCE_comm_buffer[RCCE_IAM] + RCCE_BUFF_SIZE)<=0)
    copy_mode = BOTH_IN_COMM_BUFFER;
  else 
    copy_mode = SOURCE_IN_PRIVATE_MEMORY;    

  // make sure that if the copy is between locations within the same MPB
  // there is no overlap between source and target address ranges  
  if ( copy_mode == BOTH_IN_COMM_BUFFER) {
    if (((source-target)>0 && (source+num_bytes-target)<0) ||
       ((target-source)>0 && (target+num_bytes-source)<0)) {
      return(RCCE_error_return(RCCE_debug_comm,RCCE_ERROR_DATA_OVERLAP));
    }
  }

  // ascertain that the start of the buffer is cache line aligned   
  int start_index = target-RCCE_comm_buffer[ID];
  if (start_index%RCCE_LINE_SIZE!=0) 
    return(RCCE_error_return(RCCE_debug_comm,RCCE_ERROR_ALIGNMENT));

  // only verify alignment of the target if it is in the MPB
  if (copy_mode == BOTH_IN_COMM_BUFFER) {
    start_index = source-RCCE_comm_buffer[ID];
    if (start_index%RCCE_LINE_SIZE!=0) 
      return(RCCE_error_return(RCCE_debug_comm,RCCE_ERROR_ALIGNMENT));
  }
#else
  // in non-GORY mode we only need to retain the MPB target shift; we
  // already know the target is in the MPB, not private memory
  target = RCCE_comm_buffer[ID]+(target-RCCE_comm_buffer[RCCE_IAM]);    
#endif

  // make sure that any data that has been put in our MPB by another UE is visible 
#ifdef _OPENMP
  #pragma omp flush
#endif

  // do the actual copy 
  RC_cache_invalidate();

  RCCE_memcpy_put((void *)target, (void *)source, num_bytes);

  // flush data to make it visible to all threads; cannot use flush list because it 
  // concerns malloced space                        
#ifdef _OPENMP
  #pragma omp flush
#endif

#ifdef USE_FLAG_EXPERIMENTAL
  if(RCCE_debug_synch)  
    fprintf(STDERR,"UE %d put data: %d address %X \n", RCCE_IAM,*source,target);
#endif

  return(RCCE_SUCCESS);
}

#ifdef USE_FLAG_EXPERIMENTAL
int RCCE_put_flag(
  t_vcharp target, // target buffer, MPB
  t_vcharp source, // source buffer, MPB or private memory
  int num_bytes,
  int ID
  ) {

  target = RCCE_flag_buffer[ID]+(target-RCCE_comm_buffer[RCCE_IAM]);
  
  if (RCCE_debug_synch) 
    fprintf(STDERR,"UE %d put flag: %x address %X \n", RCCE_IAM,*source,target);  
  
  //if( num_bytes == 1 ) {
    *target = *source;
    return(RCCE_SUCCESS);
  //}

  //RCCE_memcpy_put((void *)target, (void *)source, 1);

  //*RCCE_fool_write_combine_buffer = 1;
  //return(RCCE_ERROR_DEBUG_FLAG);
}
#endif
