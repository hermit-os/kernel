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
//    [2010-10-25] added support for non-blocking send/recv operations
//                 - iRCCE_isend(), ..._test(), ..._wait(), ..._push()
//                 - iRCCE_irecv(), ..._test(), ..._wait(), ..._push()
//                 by Carsten Clauss, Chair for Operating Systems,
//                                    RWTH Aachen University
//
//    [2010-11-12] extracted non-blocking code into separate library
//                 by Carsten Scholtes
//
//    [2011-01-21] updated the datatype of RCCE_FLAG according to the
//                 recent version of RCCE
//
//    [2011-04-12] added marco test for rcce version
//
//    [2012-11-06] add barrier implementation as described in:
//                 USENIX HotPar'12 Eval. Hardw. Synch. Supp. SCC
//                 by Pablo Reble  
//
#include "iRCCE_lib.h"

#ifdef SINGLEBITFLAGS
#warning iRCCE_TAGGED_FLAGS: for using this feature, SINGLEBITFLAGS must be disabled! (make SINGLEBITFLAGS=0)
#endif

#ifdef SINGLEBITFLAGS

int iRCCE_test_flag(RCCE_FLAG flag, RCCE_FLAG_STATUS val, int *result) {

	t_vcharp cflag;

#ifdef RCCE_VERSION
	// this is a newer version than V1.0.13
	t_vcharp flaga;
#endif

	cflag = flag.line_address;

#ifdef RCCE_VERSION
	// this is a newer version than V1.0.13
	flaga = flag.flag_addr;
#endif

	// always flush/invalidate to ensure we read the most recent value of *flag
	// keep reading it until it has the required value 

#ifdef _OPENMP
#pragma omp flush  
#endif
	RC_cache_invalidate();

#ifdef RCCE_VERSION
	// this is a newer version than V1.0.13
	if(RCCE_bit_value(flaga, (flag.location)%RCCE_FLAGS_PER_BYTE) != val) {
#else
	if(RCCE_bit_value(cflag, flag.location) != val) {
#endif
		(*result) = 0;
	}    
	else {
		(*result) = 1;
	}

	return(iRCCE_SUCCESS);
} 

#else

//////////////////////////////////////////////////////////////////
// LOCKLESS SYNCHRONIZATION USING ONE WHOLE CACHE LINE PER FLAG //
//////////////////////////////////////////////////////////////////

int iRCCE_test_flag(RCCE_FLAG flag, RCCE_FLAG_STATUS val, int *result) {

#ifndef RCCE_VERSION
  RCCE_FLAG flag_pos = flag;
#endif

#ifdef _OPENMP
#pragma omp flush   
#endif

  RC_cache_invalidate();

#ifdef RCCE_VERSION
  if((RCCE_FLAG_STATUS)(*flag.flag_addr) != val) {
#else
  if((*flag_pos) != val) {
#endif
    (*result) = 0;
  }    
  else {
    (*result) = 1;
  }

  return(iRCCE_SUCCESS);
}


//////////////////////////////////////////////////////////////////////////
// FUNCTIONS FOR HANDLING TAGGED FLAGS (NEED WHOLE CACHE LINE PER FLAG) //
//////////////////////////////////////////////////////////////////////////

int iRCCE_flag_alloc_tagged(RCCE_FLAG *flag)
{
#ifdef RCCE_VERSION
  // this is a newer version than V1.0.13
  flag->flag_addr = RCCE_malloc(RCCE_LINE_SIZE);
  if (!(flag->flag_addr)) return(RCCE_error_return(RCCE_debug_synch,RCCE_ERROR_FLAG_UNDEFINED));
  return(RCCE_SUCCESS);
#else
  return RCCE_flag_alloc(flag);
#endif
}

int iRCCE_flag_write_tagged(RCCE_FLAG *flag, RCCE_FLAG_STATUS val, int ID, void *tag, int len) {

  unsigned int val_array[RCCE_LINE_SIZE / sizeof(int)] = {[0 ... RCCE_LINE_SIZE/sizeof(int)-1] = 0};

  int error, i, j;

  *val_array = val;
#ifdef _OPENMP
  val_array[RCCE_LINE_SIZE/sizeof(int)-1] = val;
#endif

  if(tag)
  {
    if(len > iRCCE_MAX_TAGGED_LEN) len = iRCCE_MAX_TAGGED_LEN;
    iRCCE_memcpy_put(&val_array[sizeof(int)], tag, len);
  }

#ifdef RCCE_VERSION
  error = iRCCE_put(flag->flag_addr, (t_vcharp)val_array, RCCE_LINE_SIZE, ID);
#else
  error = iRCCE_put((t_vcharp)(*flag), (t_vcharp)val_array, RCCE_LINE_SIZE, ID);
#endif

  return(RCCE_error_return(RCCE_debug_synch,error));
}

int iRCCE_flag_read_tagged(RCCE_FLAG flag, RCCE_FLAG_STATUS *val, int ID, void *tag, int len) {

  int val_array[RCCE_LINE_SIZE / sizeof(int)];
  int error, i, j;

#ifdef RCCE_VERSION
  if((error=iRCCE_get((t_vcharp)val_array, flag.flag_addr, RCCE_LINE_SIZE, ID)))
    return(RCCE_error_return(RCCE_debug_synch,error));
#else
  if((error=iRCCE_get((t_vcharp)val_array, (t_vcharp)flag, RCCE_LINE_SIZE, ID)))
    return(RCCE_error_return(RCCE_debug_synch,error));
#endif

  if(val) *val = *val_array;

#ifdef _OPENMP
  if(val) *val = val_array[RCCE_LINE_SIZE / sizeof(int) - 1];
#endif

  if( (val) && (*val) && (tag) ) {
    if(len > iRCCE_MAX_TAGGED_LEN) len = iRCCE_MAX_TAGGED_LEN;
    iRCCE_memcpy_put(tag, &val_array[1], len);
  }

  return(RCCE_SUCCESS);
}

int iRCCE_wait_tagged(RCCE_FLAG flag, RCCE_FLAG_STATUS val, void *tag, int len) {

  int i, j;

#ifndef RCCE_VERSION
  RCCE_FLAG flag_pos = flag;
#ifdef _OPENMP
  flag_pos = flag + RCCE_LINE_SIZE / sizeof(int) - 1;
#endif
#endif

  do {
#ifdef _OPENMP
#pragma omp flush   
#endif
    RC_cache_invalidate();
#ifdef RCCE_VERSION
    // this is a newer version than V1.0.13
#ifdef _OPENMP
  } while ((RCCE_FLAG_STATUS)(*( ((int*)flag.flag_addr) + RCCE_LINE_SIZE / sizeof(int) - 1)) != val);
#else
  } while ((RCCE_FLAG_STATUS)(*flag.flag_addr) != val);
#endif
#else
  } while ((*flag_pos) != val);
#endif

  if(tag) {
    if(len >  iRCCE_MAX_TAGGED_LEN) len = iRCCE_MAX_TAGGED_LEN;
#ifdef RCCE_VERSION
    iRCCE_memcpy_put(tag, &((char*)flag.flag_addr)[sizeof(int)], len);
#else
    iRCCE_memcpy_put(tag, &((char*)flag)[sizeof(int)], len);
#endif
  }

  return(RCCE_SUCCESS);
}

int iRCCE_test_tagged(RCCE_FLAG flag, RCCE_FLAG_STATUS val, int *result, void *tag, int len) {

  int i, j;

#ifndef RCCE_VERSION
  RCCE_FLAG flag_pos = flag;
#ifdef _OPENMP
  flag_pos = flag + RCCE_LINE_SIZE / sizeof(int) - 1;
#endif
#endif

#ifdef _OPENMP
#pragma omp flush   
#endif

  RC_cache_invalidate();

#ifdef RCCE_VERSION
  if((RCCE_FLAG_STATUS)(*flag.flag_addr) != val) {
#else
  if((*flag_pos) != val) {
#endif
    (*result) = 0;
  }    
  else {
    (*result) = 1;
  }

  if((*result) && tag) {
    if(len >  iRCCE_MAX_TAGGED_LEN) len = iRCCE_MAX_TAGGED_LEN;
#ifdef RCCE_VERSION
    iRCCE_memcpy_put(tag, &((char*)flag.flag_addr)[sizeof(int)], len);
#else
    iRCCE_memcpy_put(tag, &((char*)flag)[sizeof(int)], len);
#endif
  }

  return(RCCE_SUCCESS);
}

int iRCCE_get_max_tagged_len(void)
{
  return iRCCE_MAX_TAGGED_LEN;
}
#endif
