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
#include "iRCCE_lib.h"

#ifdef __hermit__
#include "rte_memcpy.h"
#define memcpy_to_mpb rte_memcpy
#elif defined COPPERRIDGE || defined SCC
#include "scc_memcpy.h"
#else
#define memcpy_to_mpb memcpy
#endif

void* iRCCE_memcpy_put(void *dest, const void *src, size_t count)
{
#if defined COPPERRIDGE || defined SCC
  return memcpy_to_mpb(dest, src, count);
#else
  return memcpy(dest, src, count);
#endif
}

//--------------------------------------------------------------------------------------
// FUNCTION: iRCCE_put
//--------------------------------------------------------------------------------------
// copy data from address "source" in the local MPB or the calling UE's private memory 
// to address "target" in the remote MPB. We do not test to see if a move from the 
// calling UE's private memory stays within allocated memory                        
//--------------------------------------------------------------------------------------
int iRCCE_put(
  t_vcharp target, // target buffer, MPB
  t_vcharp source, // source buffer, MPB or private memory
  int num_bytes, 
  int ID
  ) {

  // in non-GORY mode we only need to retain the MPB target shift; we
  // already know the target is in the MPB, not private memory
  target = RCCE_comm_buffer[ID]+(target-RCCE_comm_buffer[RCCE_IAM]);    

  // make sure that any data that has been put in our MPB by another UE is visible 
#ifdef _OPENMP
  #pragma omp flush
#endif

  // do the actual copy 
  RC_cache_invalidate();

  iRCCE_memcpy_put((void *)target, (void *)source, num_bytes);

  // flush data to make it visible to all threads; cannot use flush list because it 
  // concerns malloced space                        
#ifdef _OPENMP
  #pragma omp flush
#endif
  return(iRCCE_SUCCESS);
}
