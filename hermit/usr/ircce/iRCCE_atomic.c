//***************************************************************************************
// Functions for handling Atomic Increment Registers (AIR).
//***************************************************************************************
//
// Copyright 2012, Chair for Operating Systems, RWTH Aachen University
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


#include "iRCCE_lib.h"

#ifdef AIR

//--------------------------------------------------------------------------------------
// FUNCTION: iRCCE_atomic_alloc
//--------------------------------------------------------------------------------------
// Allocates a new AIR register; returns iRCCE_ERRO if all AIRs are already allocated
//--------------------------------------------------------------------------------------
int iRCCE_atomic_alloc(iRCCE_AIR** reg)
{
  if(iRCCE_atomic_alloc_counter < 2 * RCCE_NP) {
    
    int next_reg = RC_COREID[iRCCE_atomic_alloc_counter];

    if(iRCCE_atomic_alloc_counter > RCCE_NP) next_reg += RCCE_MAXNP;

    (*reg) = &iRCCE_atomic_inc_regs[next_reg];

#ifdef _OPENMP
#pragma omp master
    {
      iRCCE_atomic_alloc_counter++;    
    }
#pragma omp barrier
#else
    iRCCE_atomic_alloc_counter++;
#endif

    iRCCE_atomic_write((*reg), 0);

    return iRCCE_SUCCESS;
  }
  else {

    return iRCCE_ERROR;
  }
}

//--------------------------------------------------------------------------------------
// FUNCTION: iRCCE_atomic_inc
//--------------------------------------------------------------------------------------
// Increments an AIR register and returns its privious content
//--------------------------------------------------------------------------------------
int iRCCE_atomic_inc(iRCCE_AIR* reg, int* value)
{
  int _value;
  if(value == NULL) value = &value;

#ifndef _OPENMP  
  (*value) = (*reg->counter);
#else
#pragma omp critical
  {
    (*value) = reg->counter;
    reg->counter++;
    reg->init = reg->counter;
  }
#endif

  return iRCCE_SUCCESS;
}

//--------------------------------------------------------------------------------------
// FUNCTION: iRCCE_atomic_read
//--------------------------------------------------------------------------------------
// Returns the current value of an AIR register
//--------------------------------------------------------------------------------------
int iRCCE_atomic_read(iRCCE_AIR* reg, int* value)
{
#ifndef _OPENMP
  (*value) = (*reg->init);
#else
#pragma omp critical
  {
    (*value) =reg->init;
  }
#endif

  return iRCCE_SUCCESS;
}

//--------------------------------------------------------------------------------------
// FUNCTION: iRCCE_atomic_write
//--------------------------------------------------------------------------------------
// Initializes an AIR register by writing a start value
//--------------------------------------------------------------------------------------
int iRCCE_atomic_write(iRCCE_AIR* reg, int value)
{
#ifndef _OPENMP
  (*reg->init) = value;
#else
#pragma omp critical
  {
    reg->init    = value;
    reg->counter = value;
  }
#endif

  return iRCCE_SUCCESS;
}


//--------------------------------------------------------------------------------------
// FUNCTION: iRCCE_barrier
//--------------------------------------------------------------------------------------
// A barrier version based on the Atomic Increment Registers (AIR); if AIRs are not
// supported, the function makes a fall-back to the common RCCE_barrier().
//--------------------------------------------------------------------------------------

static void RC_wait(int wait) {
#ifndef _OPENMP
  asm volatile( "movl %%eax,%%ecx\n\t"
                "test:nop\n\t"
                "loop test"
                : /* no output registers */
                : "a" (wait)
                : "%ecx" );
#endif
  return;
}

static int idx = 0;
static unsigned int rnd = 0;
#ifdef _OPENMP
#pragma omp threadprivate (idx, rnd)
#endif

int iRCCE_barrier(RCCE_COMM *comm)
{  
  int backoff = BACKOFF_MIN, wait, i = 0;
  int counter;

  if(comm == NULL) comm = &RCCE_COMM_WORLD;

  if (comm == &RCCE_COMM_WORLD) {

    iRCCE_atomic_inc(iRCCE_atomic_barrier[idx], &counter);
    if (counter < (comm->size-1)) 
    {
      iRCCE_atomic_read(iRCCE_atomic_barrier[idx], &counter);
      while (counter > 0)
      {
        rnd = rnd * 1103515245u + 12345u;
        wait = BACKOFF_MIN + (rnd % (backoff << i));
        RC_wait(wait);
        if (wait < BACKOFF_MAX) i++;

	iRCCE_atomic_read(iRCCE_atomic_barrier[idx], &counter);
      }
    }
    else
    {
      iRCCE_atomic_write(iRCCE_atomic_barrier[idx], 0);
    }

    idx = !idx;

    return(RCCE_SUCCESS);
  }
  else
  {
    return RCCE_barrier(comm);
  }
}

#else // !AIR

int iRCCE_barrier(RCCE_COMM *comm)
{  
  if(comm == NULL) return RCCE_barrier(&RCCE_COMM_WORLD);
  else return RCCE_barrier(comm);
}

#endif // !AIR
