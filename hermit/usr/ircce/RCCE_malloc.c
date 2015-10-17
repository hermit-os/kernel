//***************************************************************************************
// MPB memory allocation routines. 
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

//......................................................................................
// GLOBAL VARIABLES USED BY THE LIBRARY
//......................................................................................
static RCCE_BLOCK_S RCCE_space;   // data structure used for trscking MPB memory blocks
static RCCE_BLOCK_S *RCCE_spacep; // pointer to RCCE_space
#ifdef _OPENMP
#pragma omp threadprivate (RCCE_space, RCCE_spacep)
#endif

// END GLOBAL VARIABLES USED BY THE LIBRARY
//......................................................................................

//--------------------------------------------------------------------------------------
// FUNCTION: RCCE_malloc_init
//--------------------------------------------------------------------------------------
// initialize memory allocator
//--------------------------------------------------------------------------------------
void RCCE_malloc_init(
  t_vcharp mem, // pointer to MPB space that is to be managed by allocator
  size_t size   // size (bytes) of managed space
) {

#ifndef GORY

  // in the simplified API MPB memory allocation merely uses running pointers
  RCCE_flags_start = mem;
  RCCE_chunk       = size;
  RCCE_buff_ptr    = mem;

#else

  // create one block containing all memory for truly dynamic memory allocator
  RCCE_spacep = &RCCE_space;
  RCCE_spacep->tail = (RCCE_BLOCK *) malloc(sizeof(RCCE_BLOCK));
  RCCE_spacep->tail->free_size = size;
  RCCE_spacep->tail->space = mem;
  /* make a circular list by connecting tail to itself */
  RCCE_spacep->tail->next = RCCE_spacep->tail;

#endif
}

//--------------------------------------------------------------------------------------
// FUNCTION: RCCE_malloc
//--------------------------------------------------------------------------------------
// Allocate memory inside MPB. In restricted mode we only use it to allocate new
// flags prompted by the creation of new communicators. Since communicators are never
// deleted, we do not need to deallocate MPB memory, so we can simply keep running
// pointers of where the next flag will be stored, and where payload data can go. In
// GORY mode we need to support fully dynamic memory allocation and deallocation.
//--------------------------------------------------------------------------------------
t_vcharp RCCE_malloc(
  size_t size // requested space
) {

  t_vcharp result;

#ifndef GORY

  // new flag takes exactly one cache line, whether using single bit flags are not
  if (size != RCCE_LINE_SIZE) {
    fprintf(stderr, "ERROR in RCCE_malloc(): size != RCCE_LINE_SIZE!\n");
    exit(-1);
    return(0);
  }

  // if chunk size becomes zero, we have allocated too many flags
  if (!(RCCE_chunk-RCCE_LINE_SIZE)) {
    fprintf(stderr, "ERROR in RCCE_malloc(): No more MPB space left!\n");
    exit(-1);
    return(0);
  }

  result = RCCE_flags_start;

  // reduce maximum size of message payload chunk
  RCCE_chunk       -= RCCE_LINE_SIZE;

  // move running pointer to next available flags line
  RCCE_flags_start += RCCE_LINE_SIZE;

  // move running pointer to new start of payload data area
  RCCE_buff_ptr    += RCCE_LINE_SIZE;
  return(result);

#else

  // simple memory allocator, loosely based on public domain code developed by
  // Michael B. Allen and published on "The Scripts--IT /Developers Network".
  // Approach: 
  // - maintain linked list of pointers to memory. A block is either completely
  //   malloced (free_size = 0), or completely free (free_size > 0).
  //   The space field always points to the beginning of the block
  // - malloc: traverse linked list for first block that has enough space    
  // - free: Check if pointer exists. If yes, check if the new block should be 
  //         merged with neighbors. Could be one or two neighbors.

  RCCE_BLOCK *b1, *b2, *b3;   // running pointers for blocks              

  if (size==0 || size%RCCE_LINE_SIZE!=0) return 0;

  // always first check if the tail block has enough space, because that
  // is the most likely. If it does and it is exactly enough, we still
  // create a new block that will be the new tail, whose free space is 
  // zero. This acts as a marker of where free space of predecessor ends   
  b1 = RCCE_spacep->tail;
  if (b1->free_size >= size) {
    // need to insert new block; new order is: b1->b2 (= new tail)         
    b2 = (RCCE_BLOCK *) malloc(sizeof(RCCE_BLOCK));
    b2->next      = b1->next;
    b1->next      = b2;
    b2->free_size = b1->free_size-size;
    b2->space     = b1->space + size;
    b1->free_size = 0;
    // need to update the tail                                             
    RCCE_spacep->tail = b2;
    return(b1->space);
  }

  // tail didn't have enough space; loop over whole list from beginning    
  while (b1->next->free_size < size) {
    if (b1->next == RCCE_spacep->tail) {
      return NULL; // we came full circle 
    }
    b1 = b1->next;
  }

  b2 = b1->next;
  if (b2->free_size > size) { // split block; new block order: b1->b2->b3  
    b3            = (RCCE_BLOCK *) malloc(sizeof(RCCE_BLOCK));
    b3->next      = b2->next; // reconnect pointers to add block b3        
    b2->next      = b3;       //     "         "     "  "    "    "        
    b3->free_size = b2->free_size - size; // b3 gets remainder free space  
    b3->space     = b2->space + size; // need to shift space pointer       
  } 
  b2->free_size = 0;          // block b2 is completely used               
  return (b2->space);
#endif
}


t_vcharp RCCE_palloc(
  size_t size,        // requested space
  int CoreID // location
) {

  t_vcharp result = RCCE_malloc(size);

  if (result)
    result = RCCE_comm_buffer[CoreID]+(result-RCCE_comm_buffer[RCCE_IAM]);

  return result;
}

//--------------------------------------------------------------------------------------
// FUNCTION: RCCE_free
//--------------------------------------------------------------------------------------
// Deallocate memory in MPB; only used in GORY mode
//--------------------------------------------------------------------------------------
void RCCE_free(
  t_vcharp ptr // pointer to data to be freed
  ) {

  RCCE_BLOCK *b1, *b2, *b3;   // running block pointers                    
  int j1, j2;                 // booleans determining merging of blocks    

  // loop over whole list from the beginning until we locate space ptr     
  b1 = RCCE_spacep->tail;
  while (b1->next->space != ptr && b1->next != RCCE_spacep->tail) { 
    b1 = b1->next;
  }

  // b2 is target block whose space must be freed    
  b2 = b1->next;              
  // tail either has zero free space, or hasn't been malloc'ed             
  if (b2 == RCCE_spacep->tail) return;      

  // reset free space for target block (entire block)                      
  b3 = b2->next;
  b2->free_size = b3->space - b2->space;

  // determine with what non-empty blocks the target block can be merged   
  j1 = (b1->free_size>0 && b1!=RCCE_spacep->tail); // predecessor block    
  j2 = (b3->free_size>0 || b3==RCCE_spacep->tail); // successor block      

  if (j1) {
    if (j2) { // splice all three blocks together: (b1,b2,b3) into b1      
      b1->next = b3->next;
      b1->free_size +=  b3->free_size + b2->free_size;
      if (b3==RCCE_spacep->tail) RCCE_spacep->tail = b1;
      free(b3);
    } 
    else {    // only merge (b1,b2) into b1                                
      b1->free_size += b2->free_size;
      b1->next = b3;
    }
    free(b2);
  } 
  else {
    if (j2) { // only merge (b2,b3) into b2                                
      b2->next = b3->next;
      b2->free_size += b3->free_size;
      if (b3==RCCE_spacep->tail) RCCE_spacep->tail = b2;
      free(b3);
    } 
  }
}

//--------------------------------------------------------------------------------------
// FUNCTION: RCCE_malloc_request
//--------------------------------------------------------------------------------------
// this function tries to return a (padded) amount of space in the MPB of size 
// "size" bytes. If not available, the function keeps halving space until it fits 
//--------------------------------------------------------------------------------------
t_vcharp RCCE_malloc_request(
  size_t size,  // requested number of bytes
  size_t *chunk // number of bytes of space returned
  ) {
  
  t_vcharp combuf;

  combuf = 0;
  *chunk = PAD32byte(size);
  while (!combuf && *chunk >= RCCE_LINE_SIZE) {
    combuf = RCCE_malloc(*chunk);
    if (!combuf) *chunk = PAD32byte(*chunk/2);
  }
  return (combuf);
}
