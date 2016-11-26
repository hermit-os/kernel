//***************************************************************************************
// Communicator manipulation and accessor routines. 
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
#include "RCCE_lib.h"

#ifdef __hermit__
#include "rte_memcpy.h"
#define RCCE_memcpy_put(a,b,c) rte_memcpy(a,b,c)
#elif defined(COPPERRIDGE)
#define RCCE_memcpy_put(a,b,c) memcpy_to_mpb(a, b, c)
#include "scc_memcpy.h"
#else
#define RCCE_memcpy_put(a,b,c) memcpy(a, b, c)
#endif

#ifdef USE_RCCE_COMM
#ifndef GORY
#include "RCCE_comm/RCCE_scatter.c"
#include "RCCE_comm/RCCE_gather.c"
#include "RCCE_comm/RCCE_allgather.c"
#endif
#endif

//--------------------------------------------------------------------------------------
// FUNCTION: RCCE_comm_split
// RCCE_comm_split works like MPI_Comm_split, but:
// 1. Always uses the default global communicator as the basis, not an 
//    arbitrary communicator       
// 2. Uses the rank of the UE in the global communicator as the key
// 3. Uses a function, operating on UE's global rank, to compute color
//--------------------------------------------------------------------------------------
int RCCE_comm_split(
  int (*color)(int, void *), // function returning a color value for given ue and aux
  void *aux,                 // optional user-supplied data structure 
  RCCE_COMM *comm            // new communicator
  ) {

  int i, my_color, error;

  if (!comm) return(RCCE_error_return(RCCE_debug_comm,RCCE_ERROR_COMM_UNDEFINED));

  // start with a barrier to make sure all UEs are participating, unless we are still 
  // defining the global communicator; there is no danger in skipping the barrier in 
  // that case, because the global communicator is defined in RCCE_init, which must be 
  // called by all cores before any other RCCE calls
  if (comm != &RCCE_COMM_WORLD) RCCE_barrier(&RCCE_COMM_WORLD);
 
  // determine the size of the communicator                              
  my_color = color(RCCE_IAM, aux);

  comm->size = 0;
  for (i=0; i<RCCE_NP; i++) {
    if (color(i, aux) == my_color) {
      if (i == RCCE_IAM) comm->my_rank = comm->size;
      comm->member[comm->size++] = i;
    }
  }

  // note: we only need to allocate new synch flags if the communicator has not yet been
  // initialized. It is legal to overwrite an initialized communcator, in which case the 
  // membership may change, but the same synchronization flags can be used       
  if (comm->initialized == RCCE_COMM_INITIALIZED) return(RCCE_SUCCESS);

#ifndef USE_FAT_BARRIER
  if((error=RCCE_flag_alloc(&(comm->gather))))
    return(RCCE_error_return(RCCE_debug_comm,error));
#else
  for (i=0; i<RCCE_NP; i++) {
    if((error=RCCE_flag_alloc(&(comm->gather[i]))))
      return(RCCE_error_return(RCCE_debug_comm,error));
  }
#endif

  if((error=RCCE_flag_alloc(&(comm->release))))
     return(RCCE_error_return(RCCE_debug_comm,error));

  comm->label = 0;

  comm->initialized = RCCE_COMM_INITIALIZED;

  return(RCCE_SUCCESS);
}

// DO NOT USE THIS FUNCTION IN NON-GORY MODE UNTIL MALLOC_FREE HAS BEEN IMPLEMENTED
int RCCE_comm_free(RCCE_COMM *comm) {
  printf("DO NOT USE IN NON-GORY MODE UNTIL MALLOC_FREE HAS BEEN IMPLEMENTED\n");
  if (comm->initialized != RCCE_COMM_INITIALIZED) 
             return(RCCE_error_return(RCCE_debug_comm,RCCE_ERROR_COMM_INITIALIZED));

#ifndef USE_FAT_BARRIER
  RCCE_flag_free(&(comm->gather));
#else
  { int i;
    for (i=0; i<RCCE_NP; i++)
      RCCE_flag_free(&(comm->gather[i]));
  }
#endif

  RCCE_flag_free(&(comm->release));
  comm->initialized = RCCE_COMM_NOT_INITIALIZED;  

  return(RCCE_SUCCESS);
}  

//--------------------------------------------------------------------------------------
// FUNCTION: RCCE_comm_size
// returns the number of UEs inside the communicator
//--------------------------------------------------------------------------------------
int RCCE_comm_size(
  RCCE_COMM comm, // communicator
  int *size       // return value (size)
  ) {

  if (comm.initialized == RCCE_COMM_INITIALIZED) {
    *size = comm.size;
    return(RCCE_SUCCESS);
  }
  else return(RCCE_error_return(RCCE_debug_comm,RCCE_ERROR_COMM_INITIALIZED));
}

//--------------------------------------------------------------------------------------
// FUNCTION: RCCE_comm_rank
// returns the rank of the calling UE inside the communicator
//--------------------------------------------------------------------------------------
int RCCE_comm_rank(
  RCCE_COMM comm, // communicator
  int *rank       // return value (rank)
  ) {

  if (comm.initialized == RCCE_COMM_INITIALIZED) {
    *rank = comm.my_rank;
    return(RCCE_SUCCESS);
  }
  else return(RCCE_error_return(RCCE_debug_comm,RCCE_ERROR_COMM_INITIALIZED));
}

//--------------------------------------------------------------------------------------
// FUNCTION: RCCE_global_color
// use this trivial color function to define global communicator         
//--------------------------------------------------------------------------------------
int RCCE_global_color(int rank, void *nothing) {return(1);}
