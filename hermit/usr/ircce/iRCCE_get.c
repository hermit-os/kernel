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
#include "iRCCE_lib.h"

#ifdef __hermit__
#include "rte_memcpy.h"
#elif defined COPPERRIDGE || defined SCC
#include "scc_memcpy.h"
#endif

void* iRCCE_memcpy_get(void *dest, const void *src, size_t count)
{
#ifdef __hermit__
  return rte_memcpy(dest, src, count);
#elif defined COPPERRIDGE || defined SCC
  return memcpy_from_mpb(dest, src, count);
#else
  return memcpy(dest, src, count);
#endif
}

//--------------------------------------------------------------------------------------
// FUNCTION: iRCCE_get
//--------------------------------------------------------------------------------------
// copy data from address "source" in the remote MPB to address "target" in either the
// local MPB, or in the calling UE's private memory. We do not test to see if a move
// into the calling UE's private memory stays within allocated memory                     *
//--------------------------------------------------------------------------------------
int iRCCE_get(
  t_vcharp target, // target buffer, MPB or private memory
  t_vcharp source, // source buffer, MPB
  int num_bytes,   // number of bytes to copy (must be multiple of cache line size
  int ID           // rank of source UE
  ) {

  // in non-GORY mode we only need to retain the MPB source shift; we
  // already know the source is in the MPB, not private memory
  source = RCCE_comm_buffer[ID]+(source-RCCE_comm_buffer[RCCE_IAM]);
  
  // do the actual copy, making sure we copy fresh data                  
#ifdef _OPENMP
  #pragma omp flush
#endif
  RC_cache_invalidate();

  iRCCE_memcpy_get((void *)target, (void *)source, num_bytes);

  // flush data to make sure it is visible to all threads; cannot use a flush list 
  // because it concerns malloced space                     
#ifdef _OPENMP
  #pragma omp flush
#endif
  return(iRCCE_SUCCESS);
}
