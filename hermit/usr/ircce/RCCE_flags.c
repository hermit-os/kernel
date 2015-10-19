//**************************************************************************************
// Flag manipulation and access functions. 
// Single-bit and whole-cache-line flags are sufficiently different that we provide
// separate implementations of all the flag routines for each case
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
//    [2012-09-07] added support for "tagged" flags
//                 by Carsten Clauss, Chair for Operating Systems,
//                                    RWTH Aachen University
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
#include "RCCE_byte_flags.c"
#else

#ifdef SINGLEBITFLAGS

//////////////////////////////////////////////////////////////////
// LOCKING SYNCHRONIZATION USING ONE BIT PER FLAG 
//////////////////////////////////////////////////////////////////


//......................................................................................
// GLOBAL VARIABLES USED BY THE LIBRARY
//......................................................................................
// single bit flags are accessed with the granularity of integers. Compute the
// number of flags per integer
int WORDSIZE = sizeof(int)*8;
int LEFTMOSTBIT = sizeof(int)*8-1;
//......................................................................................
// END GLOBAL VARIABLES USED BY THE LIBRARY
//......................................................................................

RCCE_FLAG_LINE RCCE_flags = {{[0 ... RCCE_FLAGS_PER_LINE-1] = 0}, NULL, 0, NULL};

// next three utility functions are only used by the library, not the user. We assume 
// there will never be errrors, so we do not return any error code. "location" of a 
// flag bit // inside a cache line is reckoned from the most significant (leftmost) 
// bit. Within a word, flag zero is also in the leftmost bit

//--------------------------------------------------------------------------------------
// FUNCTION: RCCE_bit_value
//--------------------------------------------------------------------------------------
// return status of single bit flag at a specific location within cache line
//--------------------------------------------------------------------------------------
#if 0
// BUGGY VERSION (by Intel):
RCCE_FLAG_STATUS RCCE_bit_value(t_vcharp line_address, int location) {
  t_vintp character = (t_vintp) (line_address + location/WORDSIZE);
  int bit_position = (LEFTMOSTBIT-(location%WORDSIZE));
  unsigned int mask = 1<<bit_position;
  return (((*character) & mask)>>bit_position);
}
#else
// FIXED VERSION (by LfBS):
RCCE_FLAG_STATUS RCCE_bit_value(t_vcharp line_address, int location) {
  t_vcharp character = (t_vcharp) (line_address + location/8);
  int bit_position = 7 - location%8;
  unsigned char mask = 1<<bit_position;
  return (((*character) & mask)>>bit_position);
}
#endif

//--------------------------------------------------------------------------------------
// FUNCTION: RCCE_flip_bit_value
//--------------------------------------------------------------------------------------
// flip single bit in cache line and return value of changed bit. The location is that 
// of the bit inside the line. To find the word it is in, divide by WORDSIZE.    
//--------------------------------------------------------------------------------------
#if 0
// BUGGY VERSION (by Intel):
RCCE_FLAG_STATUS RCCE_flip_bit_value(t_vcharp line_address, int location) {
  t_vintp character = (t_vintp) (line_address + location/WORDSIZE);
  int bit_position = (LEFTMOSTBIT-(location%WORDSIZE));
  unsigned int mask = 1<<bit_position;
  (*character) ^= mask;
  return ((mask & (*character))>>bit_position);
}
#else
// FIXED VERSION (by LfBS):
RCCE_FLAG_STATUS RCCE_flip_bit_value(t_vcharp line_address, int location) {
  t_vcharp character = (t_vcharp) (line_address + location/8);
  int bit_position = 7 - location%8;
  unsigned char mask = 1<<bit_position;
  (*character) ^= mask;
  return ((mask & (*character))>>bit_position);
}
#endif

//--------------------------------------------------------------------------------------
// FUNCTION: RCCE_write_bit_value
//--------------------------------------------------------------------------------------
// write single bit in cache line and return value of changed bit. The location is that 
// of the bit inside the line. To find the word it is in, divide by WORDSIZE.    
//--------------------------------------------------------------------------------------
#if 0
// BUGGY VERSION (by Intel):
int RCCE_write_bit_value(t_vcharp line_address, int location, RCCE_FLAG_STATUS val) {
  t_vintp character = (t_vintp)(line_address + location/WORDSIZE);
  int bit_position = (LEFTMOSTBIT-(location%WORDSIZE));
  unsigned int mask;
  switch (val) {
    case RCCE_FLAG_UNSET: mask = ~(1<<bit_position);
                          (*character) &= mask;
                          break;
    case RCCE_FLAG_SET:   mask = 1<<bit_position;
                          (*character) |= mask;
                          break;
  }
  return (RCCE_SUCCESS);
}
#else
// FIXED VERSION (by LfBS):
int RCCE_write_bit_value(t_vcharp line_address, int location, RCCE_FLAG_STATUS val) {
  t_vcharp character = (t_vcharp)(line_address + location/8);
  int bit_position = 7 - location%8;
  unsigned char mask;
  switch (val) {
    case RCCE_FLAG_UNSET: mask = ~(1<<bit_position);
                          (*character) &= mask;
                          break;
    case RCCE_FLAG_SET:   mask = 1<<bit_position;
                          (*character) |= mask;
                          break;
  }
  return (RCCE_SUCCESS);
}
#endif

//--------------------------------------------------------------------------------------
// FUNCTION: RCCE_flag_alloc
//--------------------------------------------------------------------------------------
// allocate space for single bit flag. Since 256 fit on a single cache line, we only
// need to allocate new MPB space when the all existing lines are completely filled. A
// flag line is a data structure that contains an array of size RCCE_LINE_SIZE 
// characters called "flag." Each bit in field "flag" corresponds to a flag being in use 
// (bit is 1) or not (bit is 0). The actual value of the flag is stored in the MPB
// line pointed to be the field "line_address," at the corresponding bit location as in
// field "flag."
//--------------------------------------------------------------------------------------
int RCCE_flag_alloc(RCCE_FLAG *flag) {
  RCCE_FLAG_LINE *flagp;
  int c, loc;

  // find the head of the data structure that administers the flag variables
  flagp = &RCCE_flags;
  while (flagp->members == 256 && flagp->next) {
    flagp = flagp->next;
  }

  // if this is a new flag line, need to allocate MPB for it 
  if (!flagp->line_address) flagp->line_address = RCCE_malloc(RCCE_LINE_SIZE);
  if (!flagp->line_address) return(RCCE_error_return(RCCE_debug_synch,
                                   RCCE_ERROR_FLAG_NOT_ALLOCATED));

  if (flagp->members < 256) {
    // there is space in this line for a new flag; find first open slot    
    for (loc=0; loc<RCCE_LINE_SIZE*8; loc++) 
    if (!RCCE_bit_value((t_vcharp)(flagp->flag),loc)) {
      RCCE_flip_bit_value((t_vcharp)(flagp->flag),loc);
      flagp->members++;
      flag->location = loc;
      flag->line_address = flagp->line_address;
      return(RCCE_SUCCESS);
    }
  }
  else {
    // must create new flag line if last one was full
    flagp->next = (RCCE_FLAG_LINE *) malloc(sizeof(RCCE_FLAG_LINE));
    if (!(flagp->next)) return(RCCE_error_return(RCCE_debug_synch,
                                   RCCE_ERROR_FLAG_NOT_ALLOCATED));
    flagp = flagp->next;
    flagp->line_address = RCCE_malloc(RCCE_LINE_SIZE);
    if (!(flagp->line_address)) return(RCCE_error_return(RCCE_debug_synch,
                                   RCCE_ERROR_FLAG_NOT_ALLOCATED));
    // initialize the flag line 
    flagp->members=1;
    flagp->next = NULL;
    for (c=0; c<RCCE_LINE_SIZE; c++) flagp->flag[c] &= (unsigned int) 0;
    
    // flip the very first bit field to indicate that flag is not in use
    RCCE_flip_bit_value((t_vcharp)(flagp->flag),0);
    flag->location = 0;
    flag->line_address = flagp->line_address;
  } 
  return(RCCE_SUCCESS);
}

//--------------------------------------------------------------------------------------
// FUNCTION: RCCE_flag_free
//--------------------------------------------------------------------------------------
// free space for single bit flag. Since 256 fit on a single cache line, we only
// need to free claimed MPB space when the all existing lines are completely emptied.
//--------------------------------------------------------------------------------------
int RCCE_flag_free(RCCE_FLAG *flag) {

  RCCE_FLAG_LINE *flagp, *flagpminus1 = NULL;

  // check wether flag exists, and whether the location field is valid 
  if (!flag || flag->location < 0) 
    return(RCCE_error_return(RCCE_debug_synch,RCCE_ERROR_FLAG_UNDEFINED));
  // find flag line in globally maintained structure                   
  flagp  = &RCCE_flags;
  while (flagp->next && flag->line_address != flagp->line_address) {
    flagpminus1 = flagp;
    flagp = flagp->next;
  }
  if (flag->line_address != flagp->line_address) 
    return(RCCE_error_return(RCCE_debug_synch,RCCE_ERROR_FLAG_UNDEFINED));

  // error checking is done
  flagp->members--;
  RCCE_flip_bit_value((t_vcharp)(flagp->flag),flag->location);
  // something special happens if we've emptied an entire line         
  if (flagp->members==0) {
    if (flagpminus1) {
      // there is a predecessor; splice out current flag line from linked list
      RCCE_free(flagp->line_address);
      flagpminus1->next = flagp->next;
      free(flagp); 
    } 
    // if there is a successor but no predecessor, do nothing          
  }
  // invalidate location field to make sure we won't free again by mistake
  flag->location = -1;
  flag->line_address = NULL;

  return(RCCE_SUCCESS);
}

//--------------------------------------------------------------------------------------
// FUNCTION: RCCE_flag_write
//--------------------------------------------------------------------------------------
// This is the core flag manipulation routine. It requires locking to guarantee atomic
// access while updating one of a line of flags.
//--------------------------------------------------------------------------------------
int RCCE_flag_write(RCCE_FLAG *flag, RCCE_FLAG_STATUS val, int ID) {
  t_vchar val_array[RCCE_LINE_SIZE];
  int error;

#ifdef GORY
  // check input parameters 
  if (!flag || flag->location < 0 || flag->location > 255)  
    return(RCCE_error_return(RCCE_debug_synch,RCCE_ERROR_FLAG_UNDEFINED));
  if (error = (val != RCCE_FLAG_UNSET && val != RCCE_FLAG_SET))
     return(RCCE_error_return(RCCE_debug_synch,RCCE_ERROR_FLAG_STATUS_UNDEFINED));
#endif

  // acquire lock to make sure nobody else fiddles with the flags on the target core 
  RCCE_acquire_lock(ID);
  // copy entire MPB cache line containing flag to local space
  if (error = RCCE_get(val_array, flag->line_address, RCCE_LINE_SIZE, ID))
    return(RCCE_error_return(RCCE_debug_synch,error));    

  // overwrite single bit within local copy of cache line
  RCCE_write_bit_value(val_array, flag->location, val);

  // write copy back to the MPB
  error = RCCE_put(flag->line_address, val_array, RCCE_LINE_SIZE, ID);

  // release write lock for the flags on the target core 
  RCCE_release_lock(ID);
  return(RCCE_error_return(RCCE_debug_synch,error));
}

//--------------------------------------------------------------------------------------
// FUNCTION: RCCE_flag_read
//--------------------------------------------------------------------------------------
// This routine is rarely needed. We typically only read a flag when we're waiting for
// it to change value (function RCCE_wait_until). Reading does not require locking. The
// moment the target flag we're trying to read changes value, it is OK to read and
// return that value
//--------------------------------------------------------------------------------------
int RCCE_flag_read(RCCE_FLAG flag, RCCE_FLAG_STATUS *val, int ID) {
  volatile unsigned char val_array[RCCE_LINE_SIZE];
  int error;

#ifdef GORY
  if (flag.location < 0 || flag.location > 255)  
    return(RCCE_error_return(RCCE_debug_synch,RCCE_ERROR_FLAG_UNDEFINED));
  if (!val)   return(RCCE_error_return(RCCE_debug_synch,RCCE_ERROR_VAL_UNDEFINED));
#endif

// Should be able to use same technique as in RCCE_wait_until, i.e., should not need 
// to copy out of MPB first. However, this function is not time critical
  if(error=RCCE_get(val_array, flag.line_address, RCCE_LINE_SIZE, ID)) 
    return(RCCE_error_return(RCCE_debug_synch,error));
  *val = RCCE_bit_value(val_array, flag.location);
  return(RCCE_SUCCESS);
}

#else

//////////////////////////////////////////////////////////////////
// LOCKLESS SYNCHRONIZATION USING ONE WHOLE CACHE LINE PER FLAG //
//////////////////////////////////////////////////////////////////

//--------------------------------------------------------------------------------------
// FUNCTION: RCCE_flag_alloc
//--------------------------------------------------------------------------------------
// there is no internal structure to whole-cache-line flags; a new flag simply means a
// newly allocated line in the MPB
//--------------------------------------------------------------------------------------
int RCCE_flag_alloc(RCCE_FLAG *flag) {
  *flag = (RCCE_FLAG) RCCE_malloc(RCCE_LINE_SIZE);
  if (!(*flag)) return(RCCE_error_return(RCCE_debug_synch,RCCE_ERROR_FLAG_UNDEFINED));
  else          return(RCCE_SUCCESS);
}

//--------------------------------------------------------------------------------------
// FUNCTION: RCCE_flag_free
//--------------------------------------------------------------------------------------
// there is no internal structure to whole-cache-line flags; deleting a flag simply 
// means deallocating line in the MPB
//--------------------------------------------------------------------------------------
int RCCE_flag_free(RCCE_FLAG *flag) {
  if (!flag) return(RCCE_error_return(RCCE_debug_synch,RCCE_ERROR_FLAG_UNDEFINED));
  else RCCE_free((t_vcharp)(*flag));
  return(RCCE_SUCCESS);
}

//--------------------------------------------------------------------------------------
// FUNCTION: RCCE_flag_write
//--------------------------------------------------------------------------------------
// This is the core flag manipulation routine. No locking required. We simple write the
// flag value into the first word of a local (private) buffer of the size of a cache
// line and copy it to the corresponding location in the NPB
// access while updating one of a line of flags.
//--------------------------------------------------------------------------------------
int RCCE_flag_write(RCCE_FLAG *flag, RCCE_FLAG_STATUS val, int ID) {
  int error;
#ifndef USE_FLAG_EXPERIMENTAL
  volatile int val_array[RCCE_LINE_SIZE/sizeof(int)] = {[0 ... RCCE_LINE_SIZE/sizeof(int)-1] = 0};

#ifdef GORY
  // check input parameters 
  if (!flag || !(*flag)) return(RCCE_error_return(RCCE_debug_synch,RCCE_ERROR_FLAG_UNDEFINED));
  if (error = (val != RCCE_FLAG_UNSET && val != RCCE_FLAG_SET))
     return(RCCE_error_return(RCCE_debug_synch,RCCE_ERROR_FLAG_STATUS_UNDEFINED));
#endif

#ifndef USE_REVERTED_FLAGS
  *val_array = val;
#else
  val_array[RCCE_LINE_SIZE/sizeof(int)-1] = val;
#endif

  error = RCCE_put((t_vcharp)(*flag), (t_vcharp)val_array, RCCE_LINE_SIZE, ID);

#else
  //*flag = val;
  volatile unsigned char value = val;

  error = RCCE_put_flag(*flag, &value, 1, ID);
#endif

  return(RCCE_error_return(RCCE_debug_synch,error));
}

#ifdef USE_TAGGED_FLAGS
int RCCE_flag_write_tagged(RCCE_FLAG *flag, RCCE_FLAG_STATUS val, int ID, void* tag, int len) {

  unsigned char val_array[RCCE_LINE_SIZE] = {[0 ... RCCE_LINE_SIZE-1] = 0};

  int error, i, j;

#ifndef USE_REVERTED_FLAGS
  *(int *) val_array = val;
#else
  *(int *) &val_array[RCCE_LINE_SIZE-sizeof(int)] = val;
#endif

  if(tag)
  {
    if( len > ( RCCE_LINE_SIZE - sizeof(int) ) ) len = RCCE_LINE_SIZE - sizeof(int);
#ifndef USE_REVERTED_FLAGS
    memcpy_scc(&val_array[sizeof(int)], tag, len);
#else
    memcpy_scc(&val_array[0], tag, len);
#endif
  }

  error = RCCE_put((t_vcharp)(*flag), val_array, RCCE_LINE_SIZE, ID);

  return(RCCE_error_return(RCCE_debug_synch,error));
}
#endif

//--------------------------------------------------------------------------------------
// FUNCTION: RCCE_flag_read
//--------------------------------------------------------------------------------------
// This routine is rarely needed. We typically only read a flag when we're waiting for
// it to change value (function RCCE_wait_until). Reading requires copying the whole 
// MPB cache line containing the flag to a private buffer and returning the first int.
//--------------------------------------------------------------------------------------
int RCCE_flag_read(RCCE_FLAG flag, RCCE_FLAG_STATUS *val, int ID) {
  int error;
#ifndef USE_FLAG_EXPERIMENTAL
  volatile int val_array[RCCE_LINE_SIZE/sizeof(int)];
#ifdef GORY
  if (!flag)  return(RCCE_error_return(RCCE_debug_synch,RCCE_ERROR_FLAG_UNDEFINED));
  if (!val)   return(RCCE_error_return(RCCE_debug_synch,RCCE_ERROR_VAL_UNDEFINED));
#endif

  if((error=RCCE_get((t_vcharp)val_array, (t_vcharp)flag, RCCE_LINE_SIZE, ID)))
    return(RCCE_error_return(RCCE_debug_synch,error));

#ifndef USE_REVERTED_FLAGS
  if(val) *val = *val_array;
#else
  if(val) *val = val_array[RCCE_LINE_SIZE/sizeof(int)-1];
#endif

#else
  volatile unsigned char value;

  if(error=RCCE_get_flag(&value, (t_vcharp)flag, 1, ID))
    return(RCCE_error_return(RCCE_debug_synch,error));  

  if(val) *val = value;

#endif

  return(RCCE_SUCCESS);
}
#ifdef USE_TAGGED_FLAGS
int RCCE_flag_read_tagged(RCCE_FLAG flag, RCCE_FLAG_STATUS *val, int ID, void *tag, int len) {

  unsigned char val_array[RCCE_LINE_SIZE];
  int error, i, j;

  if(error=RCCE_get(val_array, (t_vcharp)flag, RCCE_LINE_SIZE, ID)) 
    return(RCCE_error_return(RCCE_debug_synch,error));

#ifndef USE_REVERTED_FLAGS
  if(val) *val = *(int *)val_array;
#else
  if(val) *val = *(int *)&val_array[RCCE_LINE_SIZE-sizeof(int)];
#endif

  if( (val) && (*val) && (tag) ) {
    if( len > ( RCCE_LINE_SIZE - sizeof(int) ) ) len = RCCE_LINE_SIZE - sizeof(int);
#ifndef USE_REVERTED_FLAGS
    memcpy_scc(tag, &val_array[sizeof(int)], len);
#else
    memcpy_scc(tag, &val_array[0], len);
#endif
  }

  return(RCCE_SUCCESS);
}
#endif
#endif

#endif
