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
//    [2010-10-25] added support for non-blocking send/recv operations
//                 - iRCCE_isend(), ..._test(), ..._wait(), ..._push()
//                 - iRCCE_irecv(), ..._test(), ..._wait(), ..._push()
//                 by Carsten Clauss, Chair for Operating Systems,
//                                    RWTH Aachen University
//
//    [2010-11-12] extracted non-blocking code into separate library
//                 by Carsten Scholtes
//
//    [2011-02-21] added support for multiple incoming queues
//                 (one recv queue per remote rank)
//
//    [2011-04-19] added wildcard mechanism (iRCCE_ANY_SOURCE) for receiving
//                 a message from an arbitrary remote rank
//                 by Simon Pickartz, Chair for Operating Systems,
//                                    RWTH Aachen University
//
//    [2011-06-27] merged iRCCE_ANY_SOURCE branch with trunk (iRCCE_ANY_LENGTH)
//

#include "RCCE.h"
#if defined(SCC) && !defined(__hermit__)
#include "SCC_API.h"
#endif
#include "iRCCE_lib.h"

// send request queue
iRCCE_SEND_REQUEST* iRCCE_isend_queue;
// recv request queue
iRCCE_RECV_REQUEST* iRCCE_irecv_queue[RCCE_MAXNP];

// recv request queue for those with source = iRCCE_ANY_SOURCE
iRCCE_RECV_REQUEST* iRCCE_irecv_any_source_queue;

// global variables for for inquiring recent source rank and recent message length
int iRCCE_recent_source = -1;
int iRCCE_recent_length =  0;

#ifdef _iRCCE_ANY_LENGTH_
const int iRCCE_ANY_LENGTH = -1 >> 1;
#endif

const int iRCCE_ANY_SOURCE = -1;

#ifdef AIR
iRCCE_AIR iRCCE_atomic_inc_regs[2*RCCE_MAXNP];
int iRCCE_atomic_alloc_counter = 0;
iRCCE_AIR* iRCCE_atomic_barrier[2];
#endif

//--------------------------------------------------------------------------------------
// FUNCTION: iRCCE_init
//--------------------------------------------------------------------------------------
// initialize the library
//--------------------------------------------------------------------------------------
int iRCCE_init(void)
{
  int i;

#ifdef AIR
#ifndef _OPENMP
  int * air_base = (int *) MallocConfigReg(FPGA_BASE + 0xE000);
#endif
#endif

  for(i=0; i<RCCE_MAXNP; i++) {
    iRCCE_irecv_queue[i] = NULL;
  }

  iRCCE_isend_queue = NULL;

  iRCCE_irecv_any_source_queue = NULL;

#ifdef AIR
#ifndef _OPENMP
  // Assign and Initialize First Set of Atomic Increment Registers
  for (i = 0; i < RCCE_MAXNP; i++)
  {
    iRCCE_atomic_inc_regs[i].counter = air_base + 2*i;
    iRCCE_atomic_inc_regs[i].init = air_base + 2*i + 1;
    if(RCCE_IAM == 0)
      *iRCCE_atomic_inc_regs[i].init = 0;
  }
  // Assign and Initialize Second Set of Atomic Increment Registers
  air_base = (int *) MallocConfigReg(FPGA_BASE + 0xF000);
  for (i = 0; i < RCCE_MAXNP; i++) 
  {
    iRCCE_atomic_inc_regs[RCCE_MAXNP+i].counter = air_base + 2*i;
    iRCCE_atomic_inc_regs[RCCE_MAXNP+i].init = air_base + 2*i + 1;
    if(RCCE_IAM == 0)
      *iRCCE_atomic_inc_regs[RCCE_MAXNP+i].init = 0;
  }
#endif

  // We need two AIRs for iRCCE_barrier();
  iRCCE_atomic_alloc(&iRCCE_atomic_barrier[0]);
  iRCCE_atomic_alloc(&iRCCE_atomic_barrier[1]);
#endif

  RCCE_barrier(&RCCE_COMM_WORLD);

  return (iRCCE_SUCCESS);
}

//--------------------------------------------------------------------------------------
// Functions form the GORY RCCE interface:
//--------------------------------------------------------------------------------------
// ... (more or less) just wrapped by respective iRCCE functions
//--------------------------------------------------------------------------------------

t_vcharp iRCCE_malloc(size_t size)
{
  t_vcharp result;
  int count;

  // new flag takes exactly one cache line, whether using single bit flags or not
  if (size % RCCE_LINE_SIZE != 0) return NULL;

  // if chunk size becomes zero, we have allocated too many flags
  if (size > RCCE_chunk) return NULL;

  result = RCCE_flags_start;

  // reduce maximum size of message payload chunk
  RCCE_chunk       -= size;

  // move running pointer to next available flags line
  RCCE_flags_start += size;

  // move running pointer to new start of payload data area
  RCCE_buff_ptr    += size;

  return result;
}

int iRCCE_flag_alloc(RCCE_FLAG *flag)
{
#if !defined(SINGLEBITFLAGS)
  return iRCCE_flag_alloc_tagged(flag);
#else
  return RCCE_flag_alloc(flag);
#endif  
}

int iRCCE_flag_write(RCCE_FLAG *flag, RCCE_FLAG_STATUS val, int ID)
{
#if !defined(SINGLEBITFLAGS)
  return iRCCE_flag_write_tagged(flag, val, ID, NULL, 0);
#else
  return RCCE_flag_write(flag, val, ID);
#endif 
}

int iRCCE_flag_read(RCCE_FLAG flag, RCCE_FLAG_STATUS *val, int ID)
{
#if !defined(SINGLEBITFLAGS)
  return iRCCE_flag_read_tagged(flag, val, ID, NULL, 0);
#else
  return RCCE_flag_read(flag, val, ID);
#endif  
}

int iRCCE_wait_until(RCCE_FLAG flag, RCCE_FLAG_STATUS val)
{
#if !defined(SINGLEBITFLAGS)
  return iRCCE_wait_tagged(flag, val, NULL, 0);
#else
  return iRCCE_wait_until(flag, val);
#endif  
}
