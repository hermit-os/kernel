//***************************************************************************************
// Synchronized receive routines. 
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
//    [2010-10-25] added support for non-blocking send/recv operations
//                 - RCCE_isend(), ..._test(), ..._wait(), ..._push()
//                 - RCCE_irecv(), ..._test(), ..._wait(), ..._push()
//                 by Carsten Clauss, Chair for Operating Systems,
//                                    RWTH Aachen University
//
//    [2012-09-10] added support for "tagged" flags
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

#include <stdlib.h>
#include <string.h>

//--------------------------------------------------------------------------------------
// FUNCTION: RCCE_recv_general
//--------------------------------------------------------------------------------------
// Synchronized receive function (gory and non-gory mode)
//--------------------------------------------------------------------------------------
static int RCCE_recv_general(
  char *privbuf,    // destination buffer in local private memory (receive buffer)
  t_vcharp combuf,  // intermediate buffer in MPB
  size_t chunk,     // size of MPB available for this message (bytes)
  RCCE_FLAG *ready, // flag indicating whether receiver is ready
  RCCE_FLAG *sent,  // flag indicating whether message has been sent by source
  size_t size,      // size of message (bytes)
  int source,       // UE that sent the message
  int *test,        // if 1 upon entry, do nonblocking receive; if message available
                    // set to 1, otherwise to 0
  int copy,         // set to 0 for cancel function
  int pipe,         // use pipelining?
  int mcast,        // multicast?
  void* tag,        // additional tag?
  int len,          // length of additional tag
  RCCE_FLAG *probe  // flag for probing for incoming messages
  ) {

  char padline[RCCE_LINE_SIZE]; // copy buffer, used if message not multiple of line size
  size_t wsize,   // offset within receive buffer when pulling in "chunk" bytes
       remainder, // bytes remaining to be received
       nbytes;    // number of bytes to be received in single RCCE_get call
  int first_test; // only use first chunk to determine if message has been received yet
  char *bufptr;   // running pointer inside privbuf for current location
  RCCE_FLAG_STATUS flag;

  first_test = 1;

#ifdef USE_REMOTE_PUT_LOCAL_GET
  if(mcast) return(RCCE_error_return(1, RCCE_ERROR_NO_MULTICAST_SUPPORT));
#endif

  if(probe) {
#ifdef USE_TAGGED_FLAGS
    RCCE_wait_tagged(*probe, RCCE_FLAG_SET, tag, len);
#else
    RCCE_wait_until(*probe, RCCE_FLAG_SET);
#endif
    RCCE_flag_write(probe, RCCE_FLAG_UNSET, RCCE_IAM);
  }

#ifdef USE_SYNCH_FOR_ZERO_BYTE
  // synchronize even in case of zero byte messages:
  if(size == 0) {
#ifdef USE_REMOTE_PUT_LOCAL_GET
    RCCE_flag_write(ready, RCCE_FLAG_SET, source);
#ifdef USE_TAGGED_FLAGS
    if(!probe)
      RCCE_wait_tagged(*sent, RCCE_FLAG_SET, tag, len);
    else
#endif
    RCCE_wait_until(*sent, RCCE_FLAG_SET);
    RCCE_flag_write(sent, RCCE_FLAG_UNSET, RCCE_IAM);
#else // LOCAL PUT / REMOTE GET: (standard)
#ifdef USE_TAGGED_FLAGS
    if(!probe)
      RCCE_wait_tagged(*sent, RCCE_FLAG_SET, tag, len);
    else
#endif
    RCCE_wait_until(*sent, RCCE_FLAG_SET);      
    RCCE_flag_write(ready, RCCE_FLAG_SET, source);
#endif // !USE_REMOTE_PUT_LOCAL_GET
    return(RCCE_SUCCESS);
  }
#endif // USE_SYNCH_FOR_ZERO_BYTE

#ifdef USE_REMOTE_PUT_LOCAL_GET

  first_test = 0; /* force blocking function, does not work for now */
  *test = 1;

  // tell the source I am ready to receive
  RCCE_flag_write(ready, RCCE_FLAG_SET, source);
  
  if(!pipe) {
    // receive data in units of available chunk size of MPB 
    for (wsize=0; wsize< (size/chunk)*chunk; wsize+=chunk) {
      bufptr = privbuf + wsize;
      nbytes = chunk;
      // if function is called in test mode, check if first chunk has been sent already. 
      // If so, proceed as usual. If not, exit immediately 
      if (*test && first_test) {
	first_test = 0;
	RCCE_test_flag(*sent, RCCE_FLAG_SET, test);
	if (!(*test)) return(RCCE_SUCCESS);
      }
      
      if (wsize != 0)
	RCCE_flag_write(ready, RCCE_FLAG_SET, source);
  
#ifdef USE_TAGGED_FLAGS
      if( (wsize == 0) && (!probe) )
	RCCE_wait_tagged(*sent, RCCE_FLAG_SET, tag, len);
      else
#endif     
      RCCE_wait_until(*sent, RCCE_FLAG_SET);

      RCCE_flag_write(sent, RCCE_FLAG_UNSET, RCCE_IAM);

      // copy data from local MPB space to private memory 
      if(copy) RCCE_get((t_vcharp)bufptr, combuf, nbytes, RCCE_IAM);
    }
  }
  
#else // LOCAL PUT / REMOTE GET: (standard)

  if(!pipe) {
    // receive data in units of available chunk size of MPB 
    for (wsize=0; wsize< (size/chunk)*chunk; wsize+=chunk) {
      bufptr = privbuf + wsize;
      nbytes = chunk;
      // if function is called in test mode, check if first chunk has been sent already. 
      // If so, proceed as usual. If not, exit immediately 
      if (*test && first_test) {
	first_test = 0;
	RCCE_test_flag(*sent, RCCE_FLAG_SET, test);
	if (!(*test)) return(RCCE_SUCCESS);
      }
      if(!mcast)
      {
#ifdef USE_TAGGED_FLAGS
	if( (wsize == 0) && (!probe) )
	  RCCE_wait_tagged(*sent, RCCE_FLAG_SET, tag, len);
	else
#endif
	RCCE_wait_until(*sent, RCCE_FLAG_SET);

	RCCE_flag_write(sent, RCCE_FLAG_UNSET, RCCE_IAM);
      }
      else {
	RCCE_TNS_barrier(&RCCE_COMM_WORLD);
      }
      // copy data from remote MPB space to private memory 
      if(copy) RCCE_get((t_vcharp)bufptr, combuf, nbytes, source);
      
      if(!mcast) {
	// tell the source I have moved data out of its comm buffer
	RCCE_flag_write(ready, RCCE_FLAG_SET, source);
      }
      else {
	RCCE_TNS_barrier(&RCCE_COMM_WORLD);
      }
    }
  }
#endif // !USE_REMOTE_PUT_LOCAL_GET

#ifdef USE_REMOTE_PUT_LOCAL_GET

  else // if(!pipe) ->  if(pipe)
  { 
    // pipelined version of send/recv:

    size_t subchunk1, subchunk2; 

    for (wsize=0; wsize < (size/chunk)*chunk; wsize+=chunk) {

      if (*test && first_test) {
	first_test = 0;
	RCCE_test_flag(*sent, RCCE_FLAG_SET, test);
	if (!(*test)) return(RCCE_SUCCESS);
      }

      if(wsize == 0) {
	// allign sub-chunks to cache line granularity:
	subchunk1 = ( (chunk / 2) / RCCE_LINE_SIZE ) * RCCE_LINE_SIZE;
	subchunk2 = chunk - subchunk1;
      }

      bufptr = privbuf + wsize;
      nbytes = subchunk1;
      
#ifdef USE_TAGGED_FLAGS
      if( (wsize == 0) && (!probe) )
	RCCE_wait_tagged(*sent, RCCE_FLAG_SET, tag, len);
      else
#endif
      RCCE_wait_until(*sent, RCCE_FLAG_SET);
      
      RCCE_flag_write(sent, RCCE_FLAG_UNSET, RCCE_IAM);

      RCCE_flag_write(ready, RCCE_FLAG_SET, source);

      // copy data chunk 1 from local MPB space to private memory 
      if(copy) RCCE_get((t_vcharp)bufptr, combuf, nbytes, RCCE_IAM);

      bufptr = privbuf + wsize + subchunk1;
      nbytes = subchunk2;

      RCCE_wait_until(*sent, RCCE_FLAG_SET);
      RCCE_flag_write(sent, RCCE_FLAG_UNSET, RCCE_IAM);

      if (wsize + chunk < (size/chunk)*chunk)
        RCCE_flag_write(ready, RCCE_FLAG_SET, source);

      // copy data chunk 2 from local MPB space to private memory 
      if(copy) RCCE_get((t_vcharp)bufptr, combuf + subchunk1, nbytes, RCCE_IAM);
    }

  } //  if(pipe)

#else // LOCAL PUT / REMOTE GET: (standard)

  else // if(!pipe) ->  if(pipe)
  {
    // pipelined version of send/recv:
  
    size_t subchunk1, subchunk2;

    for (wsize=0; wsize < (size/chunk)*chunk; wsize+=chunk) {
      
      if (*test && first_test) {
	first_test = 0;
	RCCE_test_flag(*sent, RCCE_FLAG_SET, test);
	if (!(*test)) return(RCCE_SUCCESS);
      }

      if(wsize == 0) {
	// allign sub-chunks to cache line granularity:
	subchunk1 = ( (chunk / 2) / RCCE_LINE_SIZE ) * RCCE_LINE_SIZE;
	subchunk2 = chunk - subchunk1;
      }
      
      bufptr = privbuf + wsize;
      nbytes = subchunk1;
      
#ifdef USE_TAGGED_FLAGS
      if( (wsize == 0) && (!probe) )
	RCCE_wait_tagged(*sent, RCCE_FLAG_SET, tag, len);
      else
#endif
      RCCE_wait_until(*sent, RCCE_FLAG_SET);

      RCCE_flag_write(sent, RCCE_FLAG_UNSET, RCCE_IAM);
      RCCE_flag_write(ready, RCCE_FLAG_SET, source);

      // copy data chunk 1 from remote MPB space to private memory 
      if(copy) RCCE_get((t_vcharp)bufptr, combuf, nbytes, source);
      
      bufptr = privbuf + wsize + subchunk1;
      nbytes = subchunk2;
      
      RCCE_wait_until(*sent, RCCE_FLAG_SET);
      RCCE_flag_write(sent, RCCE_FLAG_UNSET, RCCE_IAM);
      RCCE_flag_write(ready, RCCE_FLAG_SET, source);

      // copy data chunk 2 from remote MPB space to private memory 
      if(copy) RCCE_get((t_vcharp)bufptr, combuf + subchunk1, nbytes, source);
    }

  } // if(pipe)

#endif // !USE_REMOTE_PUT_LOCAL_GET

  remainder = size%chunk; 
  // if nothing is left over, we are done 
  if (!remainder) return(RCCE_SUCCESS);

  // receive remainder of data--whole cache lines               
  bufptr = privbuf + (size/chunk)*chunk;
  nbytes = remainder - remainder%RCCE_LINE_SIZE;

  if (nbytes) {

    // if function is called in test mode, check if first chunk has been sent already. 
    // If so, proceed as usual. If not, exit immediately 
    if (*test && first_test) {
      first_test = 0;
      RCCE_test_flag(*sent, RCCE_FLAG_SET, test);
      if (!(*test)) return(RCCE_SUCCESS);
    }

#ifdef USE_REMOTE_PUT_LOCAL_GET

    if (wsize != 0)
      RCCE_flag_write(ready, RCCE_FLAG_SET, source);

#ifdef USE_TAGGED_FLAGS
    if( (wsize == 0) && (!probe) )
      RCCE_wait_tagged(*sent, RCCE_FLAG_SET, tag, len);
    else
#endif
    RCCE_wait_until(*sent, RCCE_FLAG_SET);

    RCCE_flag_write(sent, RCCE_FLAG_UNSET, RCCE_IAM);
    
    // copy data from local MPB space to private memory 
    if(copy) RCCE_get((t_vcharp)bufptr, combuf, nbytes, RCCE_IAM);
    wsize += nbytes;

#else // LOCAL PUT / REMOTE GET: (standard)

    if(!mcast) {
#ifdef USE_TAGGED_FLAGS
      if( (wsize == 0) && (!probe) )
	RCCE_wait_tagged(*sent, RCCE_FLAG_SET, tag, len);
      else
#endif
      RCCE_wait_until(*sent, RCCE_FLAG_SET);

      RCCE_flag_write(sent, RCCE_FLAG_UNSET, RCCE_IAM);
    }
    else {
      RCCE_TNS_barrier(&RCCE_COMM_WORLD);
    }

    // copy data from remote MPB space to private memory 
    if(copy) RCCE_get((t_vcharp)bufptr, combuf, nbytes, source);

    if(!mcast) {
      // tell the source I have moved data out of its comm buffer
      RCCE_flag_write(ready, RCCE_FLAG_SET, source);
    }
    else {
      RCCE_TNS_barrier(&RCCE_COMM_WORLD);
    }
#endif // !USE_REMOTE_PUT_LOCAL_GET

  } // if (nbytes)

  remainder = remainder%RCCE_LINE_SIZE;
  if (!remainder) return(RCCE_SUCCESS);

  // remainder is less than cache line. This must be copied into appropriately sized 
  // intermediate space before exact number of bytes get copied to the final destination 
  bufptr = privbuf + (size/chunk)*chunk + nbytes;
  nbytes = RCCE_LINE_SIZE;

  // if function is called in test mode, check if first chunk has been sent already. 
  // If so, proceed as usual. If not, exit immediately 
  if (*test && first_test) {
    first_test = 0;
    RCCE_test_flag(*sent, RCCE_FLAG_SET, test);
    if (!(*test)) return(RCCE_SUCCESS);
  }

#ifdef USE_REMOTE_PUT_LOCAL_GET

  if (wsize != 0)
    RCCE_flag_write(ready, RCCE_FLAG_SET, source);

#ifdef USE_TAGGED_FLAGS
  if( (wsize == 0) && (!probe) )
    RCCE_wait_tagged(*sent, RCCE_FLAG_SET, tag, len);
  else
#endif
  RCCE_wait_until(*sent, RCCE_FLAG_SET);

  RCCE_flag_write(sent, RCCE_FLAG_UNSET, RCCE_IAM);

  // copy data from local MPB space to private memory
  if(copy) {
    RCCE_get((t_vcharp)padline, combuf, nbytes, RCCE_IAM);
#ifdef COPPERRIDGE
    memcpy_scc(bufptr,padline,remainder);
#else
    memcpy(bufptr,padline,remainder);
#endif
  }
    
#else // LOCAL PUT / REMOTE GET: (standard)

  if(!mcast) {
#ifdef USE_TAGGED_FLAGS
    if( (wsize == 0) && (!probe) )
      RCCE_wait_tagged(*sent, RCCE_FLAG_SET, tag, len);
    else
#endif
    RCCE_wait_until(*sent, RCCE_FLAG_SET);
    
    RCCE_flag_write(sent, RCCE_FLAG_UNSET, RCCE_IAM);
  }
  else {
    RCCE_TNS_barrier(&RCCE_COMM_WORLD);
  }
  
  // copy data from remote MPB space to private memory   
  if(copy) {
    RCCE_get((t_vcharp)padline, combuf, nbytes, source);
#ifdef COPPERRIDGE
    memcpy_scc(bufptr,padline,remainder);
#else
    memcpy(bufptr,padline,remainder);
#endif
  }
  
  if(!mcast) {
    // tell the source I have moved data out of its comm buffer
    RCCE_flag_write(ready, RCCE_FLAG_SET, source);
  }
  else {
    RCCE_TNS_barrier(&RCCE_COMM_WORLD);
  }

#endif // !USE_REMOTE_PUT_LOCAL_GET

  return(RCCE_SUCCESS);
}


static int RCCE_push_recv_request(RCCE_RECV_REQUEST *request) {

  char padline[RCCE_LINE_SIZE]; // copy buffer, used if message not multiple of line size
  int   test;                   // flag for calling RCCE_test_flag()

  if(request->finished) return(RCCE_SUCCESS);

  if(request->label == 1) goto label1;
  if(request->label == 2) goto label2;
  if(request->label == 3) goto label3;
  if(request->label == 4) goto label4;

  if(request->probe) {
#ifdef USE_TAGGED_FLAGS
    RCCE_test_tagged(*(request->probe), RCCE_FLAG_SET, &test, request->tag, request->len);
#else
    RCCE_test_flag(*(request->probe), RCCE_FLAG_SET, &test);
#endif
    if(!test) {
      request->label = 0;
      return(RCCE_PENDING);
    }
    RCCE_flag_write(request->probe, RCCE_FLAG_UNSET, RCCE_IAM);
  }

#ifdef USE_SYNCH_FOR_ZERO_BYTE
  // synchronize even in case of zero byte messages:
  if(request->size == 0) {
#ifdef USE_REMOTE_PUT_LOCAL_GET
    RCCE_flag_write(request->ready, RCCE_FLAG_SET, request->source);
  label1:
#ifdef USE_TAGGED_FLAGS
    if(!request->probe)
      RCCE_test_tagged(*(request->sent), RCCE_FLAG_SET, &test, request->tag, request->len);
    else
#endif
    RCCE_test_flag(*(request->sent), RCCE_FLAG_SET, &test);
    if(!test) {
      request->label = 1;
      return(RCCE_PENDING);      
    }
    RCCE_flag_write(request->sent, RCCE_FLAG_UNSET, RCCE_IAM);
#else // LOCAL PUT / REMOTE GET: (standard)
  label1:
#ifdef USE_TAGGED_FLAGS
    if(!request->probe)
      RCCE_test_tagged(*(request->sent), RCCE_FLAG_SET, &test, request->tag, request->len);
    else
#endif
    RCCE_test_flag(*(request->sent), RCCE_FLAG_SET, &test);
    if(!test) {
      request->label = 1;
      return(RCCE_PENDING);      
    }
    RCCE_flag_write(request->sent, RCCE_FLAG_UNSET, RCCE_IAM);
    RCCE_flag_write(request->ready, RCCE_FLAG_SET, request->source);
#endif // !USE_REMOTE_PUT_LOCAL_GET
    request->finished = 1;
    return(RCCE_SUCCESS);
  }
#endif // USE_SYNCH_FOR_ZERO_BYTE


  // receive data in units of available chunk size of MPB 
  for (; request->wsize < (request->size / request->chunk) * request->chunk; request->wsize += request->chunk) {
    request->bufptr = request->privbuf + request->wsize;
    request->nbytes = request->chunk;

#ifdef USE_REMOTE_PUT_LOCAL_GET

    // tell the source I am ready to receive
    RCCE_flag_write(request->ready, RCCE_FLAG_SET, request->source);

  label2:
#ifdef USE_TAGGED_FLAGS
    if( (request->wsize == 0) && (!request->probe) )
      RCCE_test_tagged(*(request->sent), RCCE_FLAG_SET, &test, request->tag, request->len);
    else
#endif
    RCCE_test_flag(*(request->sent), RCCE_FLAG_SET, &test);

    if(!test) {
      request->label = 2;
      return(RCCE_PENDING);
    }
    RCCE_flag_write(request->sent, RCCE_FLAG_UNSET, RCCE_IAM);
    
    // copy data from local MPB space to private memory 
    if(request->copy) RCCE_get((t_vcharp)request->bufptr, request->combuf, request->nbytes, RCCE_IAM);

#else // LOCAL PUT / REMOTE GET: (standard)

  label2:
#ifdef USE_TAGGED_FLAGS
    if( (request->wsize == 0) && (!request->probe) )
      RCCE_test_tagged(*(request->sent), RCCE_FLAG_SET, &test, request->tag, request->len);
    else
#endif
    RCCE_test_flag(*(request->sent), RCCE_FLAG_SET, &test);

    if(!test) {
      request->label = 2;
      return(RCCE_PENDING);
    }
    RCCE_flag_write(request->sent, RCCE_FLAG_UNSET, RCCE_IAM);
    
    // copy data from remote MPB space to private memory 
    if(request->copy) RCCE_get((t_vcharp)request->bufptr, request->combuf, request->nbytes, request->source);

    // tell the source I have moved data out of its comm buffer
    RCCE_flag_write(request->ready, RCCE_FLAG_SET, request->source);

#endif // !USE_REMOTE_PUT_LOCAL_GET

  } // for

  request->remainder = request->size % request->chunk; 
  // if nothing is left over, we are done 
  if (!request->remainder) {
    request->finished = 1;
    return(RCCE_SUCCESS);
  }

  // receive remainder of data--whole cache lines               
  request->bufptr = request->privbuf + (request->size / request->chunk) * request->chunk;
  request->nbytes = request->remainder - request->remainder % RCCE_LINE_SIZE;

  if (request->nbytes) {

#ifdef USE_REMOTE_PUT_LOCAL_GET

    // tell the source I am ready to receive
    RCCE_flag_write(request->ready, RCCE_FLAG_SET, request->source);

  label3:
#ifdef USE_TAGGED_FLAGS
    if( (request->wsize == 0) && (!request->probe) )
      RCCE_test_tagged(*(request->sent), RCCE_FLAG_SET, &test, request->tag, request->len);
    else
#endif
    RCCE_test_flag(*(request->sent), RCCE_FLAG_SET, &test);

    if(!test) {
      request->label = 3;
      return(RCCE_PENDING);
    }
    RCCE_flag_write(request->sent, RCCE_FLAG_UNSET, RCCE_IAM);
    
    // copy data from local MPB space to private memory 
     if(request->copy) RCCE_get((t_vcharp)request->bufptr, request->combuf, request->nbytes, RCCE_IAM);

#else // LOCAL PUT / REMOTE GET: (standard)

  label3:
#ifdef USE_TAGGED_FLAGS
    if( (request->wsize == 0) && (!request->probe) )
      RCCE_test_tagged(*(request->sent), RCCE_FLAG_SET, &test, request->tag, request->len);
    else
#endif
    RCCE_test_flag(*(request->sent), RCCE_FLAG_SET, &test);

    if(!test) {
      request->label = 3;
      return(RCCE_PENDING);
    }

    RCCE_flag_write(request->sent, RCCE_FLAG_UNSET, RCCE_IAM);

    // copy data from remote MPB space to private memory 
    if(request->copy) RCCE_get((t_vcharp)request->bufptr, request->combuf, request->nbytes, request->source);

    // tell the source I have moved data out of its comm buffer
    RCCE_flag_write(request->ready, RCCE_FLAG_SET, request->source);

#endif // !USE_REMOTE_PUT_LOCAL_GET

  } // if(request->nbytes)

  request->remainder = request->size % request->chunk; 
  request->remainder = request->remainder % RCCE_LINE_SIZE;

  if (!request->remainder) {
    request->finished = 1;
    return(RCCE_SUCCESS);
  }

  // remainder is less than cache line. This must be copied into appropriately sized 
  // intermediate space before exact number of bytes get copied to the final destination 
  request->bufptr = request->privbuf + (request->size / request->chunk) * request->chunk + request->nbytes;
  request->nbytes = RCCE_LINE_SIZE;

#ifdef USE_REMOTE_PUT_LOCAL_GET

  // tell the source I am ready to receive
  RCCE_flag_write(request->ready, RCCE_FLAG_SET, request->source);

label4:
#ifdef USE_TAGGED_FLAGS
#ifdef USE_PROBE_FLAGS_SHORTCUT
  if(request->privbuf == NULL) 
  {
    request->finished = 1;
    return(RCCE_SUCCESS);
  }
#endif
  if( (request->wsize == 0) && (!request->probe) )
    RCCE_test_tagged(*(request->sent), RCCE_FLAG_SET, &test, request->tag, request->len);
  else
#endif
  RCCE_test_flag(*(request->sent), RCCE_FLAG_SET, &test);
  
  if(!test) {
    request->label = 4;
    return(RCCE_PENDING);
  }
  RCCE_flag_write(request->sent, RCCE_FLAG_UNSET, RCCE_IAM);
  
  // copy data from local MPB space to private memory 
  if(request->copy) {
    RCCE_get((t_vcharp)padline, request->combuf, request->nbytes, RCCE_IAM);  
#ifdef COPPERRIDGE
    memcpy_scc(request->bufptr,padline,request->remainder);
#else
    memcpy(request->bufptr,padline,request->remainder);
#endif
  }

#else // LOCAL PUT / REMOTE GET: (standard)

 label4:
#ifdef USE_TAGGED_FLAGS
  if( (request->wsize == 0) && (!request->probe) )
    RCCE_test_tagged(*(request->sent), RCCE_FLAG_SET, &test, request->tag, request->len);
  else
#endif
  RCCE_test_flag(*(request->sent), RCCE_FLAG_SET, &test);

  if(!test) {
    request->label = 4;
    return(RCCE_PENDING);
  }
  RCCE_flag_write(request->sent, RCCE_FLAG_UNSET, RCCE_IAM);

  // copy data from remote MPB space to private memory   
  if(request->copy) {
    RCCE_get((t_vcharp)padline, request->combuf, request->nbytes, request->source);
#ifdef COPPERRIDGE
    memcpy_scc(request->bufptr,padline,request->remainder);
#else
    memcpy(request->bufptr,padline,request->remainder);
#endif
  }
  
  // tell the source I have moved data out of its comm buffer
  RCCE_flag_write(request->ready, RCCE_FLAG_SET, request->source);

#endif // !USE_REMOTE_PUT_LOCAL_GET

  request->finished = 1;
  return(RCCE_SUCCESS);
}

static void RCCE_init_recv_request(
  char *privbuf,    // source buffer in local private memory (send buffer)
  t_vcharp combuf,  // intermediate buffer in MPB
  size_t chunk,     // size of MPB available for this message (bytes)
  RCCE_FLAG *ready, // flag indicating whether receiver is ready
  RCCE_FLAG *sent,  // flag indicating whether message has been sent by source
  size_t size,      // size of message (bytes)
  int source,       // UE that will send the message
  int copy,         // set to 0 for cancel function
  void* tag,        // additional tag?
  int len,          // length of additional tag  
  RCCE_FLAG *probe, // flag for probing for incoming messages
  RCCE_RECV_REQUEST *request
  ) {

  request->privbuf   = privbuf;
  request->combuf    = combuf;
  request->chunk     = chunk;
  request->ready     = ready;
  request->sent      = sent;
  request->size      = size;
  request->source    = source;

  request->copy      = copy;
  request->tag       = tag;
  request->len       = len;
  request->probe     = probe;

  request->wsize     = 0;
  request->remainder = 0;
  request->nbytes    = 0;
  request->bufptr    = NULL;

  request->label     = 0;
  request->finished  = 0;

  request->next      = NULL;

  return;
}

#ifndef GORY
// this is the LfBS-customized message passing API      

//--------------------------------------------------------------------------------------
// FUNCTION: RCCE_recv
//--------------------------------------------------------------------------------------
// recv function for simplified API; use library-maintained variables for synchronization
// and set the test variable to 0 (ignore)
//--------------------------------------------------------------------------------------
int RCCE_recv(char *privbuf, size_t size, int source) {
  int ignore;

#ifdef USE_PROBE_FLAGS
  RCCE_FLAG* probe = &RCCE_probe_flag[source];
#else
  RCCE_FLAG* probe = 0;
#endif

#ifndef USE_REMOTE_PUT_LOCAL_GET
  if(RCCE_recv_queue[source] != NULL)
#else
  if(RCCE_recv_queue != NULL)
#endif
    return(RCCE_REJECTED);

  ignore = 0;
#ifdef USE_TAGGED_FOR_SHORT
  if(size <= (RCCE_LINE_SIZE - sizeof(int)))
  {
#ifdef USE_PROBE_FLAGS
    RCCE_wait_tagged(*probe, RCCE_FLAG_SET, privbuf, size);
    RCCE_flag_write(probe, RCCE_FLAG_UNSET, RCCE_IAM);
#endif

#ifdef USE_REMOTE_PUT_LOCAL_GET

    RCCE_flag_write(&RCCE_ready_flag[RCCE_IAM], RCCE_FLAG_SET, source);
  
#ifndef USE_PROBE_FLAGS_SHORTCUT
#ifdef USE_PROBE_FLAGS
    RCCE_wait_until(RCCE_sent_flag[source], RCCE_FLAG_SET);
#else
    RCCE_wait_tagged(RCCE_sent_flag[source], RCCE_FLAG_SET, privbuf, size);
#endif
    RCCE_flag_write(&RCCE_sent_flag[source], RCCE_FLAG_UNSET, RCCE_IAM);
#endif

#else // LOCAL PUT / REMOTE GET: (standard)

#ifdef USE_PROBE_FLAGS
    RCCE_wait_until(RCCE_sent_flag[source], RCCE_FLAG_SET);
#else
    RCCE_wait_tagged(RCCE_sent_flag[source], RCCE_FLAG_SET, privbuf, size);
#endif
    RCCE_flag_write(&RCCE_sent_flag[source], RCCE_FLAG_UNSET, RCCE_IAM);

    RCCE_flag_write(&RCCE_ready_flag[RCCE_IAM], RCCE_FLAG_SET, source);  

#endif // !USE_REMOTE_PUT_LOCAL_GET

    return(RCCE_SUCCESS);
  }
  else
#endif
  return(RCCE_recv_general(privbuf, RCCE_buff_ptr, RCCE_chunk, 
			   &RCCE_ready_flag[RCCE_IAM], &RCCE_sent_flag[source], 
			   size, source, &ignore, 
			   1, 0, 0,          // copy, pipe, mcast
			   NULL, 0, probe)); // tag, len, probe
}

int RCCE_recv_tagged(char *privbuf, size_t size, int source, void* tag, int len) {
  int ignore;

#ifdef USE_PROBE_FLAGS
  RCCE_FLAG* probe = &RCCE_probe_flag[source];
#else
  RCCE_FLAG* probe = 0;
#endif

#ifndef USE_REMOTE_PUT_LOCAL_GET
  if(RCCE_recv_queue[source] != NULL)
#else
  if(RCCE_recv_queue != NULL)
#endif
    return(RCCE_REJECTED);

  ignore = 0;
#ifdef USE_TAGGED_FLAGS
  return(RCCE_recv_general(privbuf, RCCE_buff_ptr, RCCE_chunk, 
			   &RCCE_ready_flag[RCCE_IAM], &RCCE_sent_flag[source], 
			   size, source, &ignore, 
			   1, 0, 0,           // copy, pipe, mcast
			   tag, len, probe)); // tag, len, probe
#else
  RCCE_recv_general(tag, RCCE_buff_ptr, RCCE_chunk, 
		    &RCCE_ready_flag[RCCE_IAM], &RCCE_sent_flag[source], 
		    len, source, &ignore, 
		    1, 0, 0,         // copy, pipe, mcast
		    NULL, 0, probe); // tag, len, probe

  return(RCCE_recv_general(privbuf, RCCE_buff_ptr, RCCE_chunk, 
			   &RCCE_ready_flag[RCCE_IAM], &RCCE_sent_flag[source], 
			   size, source, &ignore, 
			   1, 0, 0,          // copy, pipe, mcast
			   NULL, 0, probe)); // tag, len, probe
#endif
}


//--------------------------------------------------------------------------------------
// FUNCTION: RCCE_recv_pipe
//--------------------------------------------------------------------------------------
// recv function for simplified API; use library-maintained variables for synchronization
// and set the test variable to 0 (ignore)
//--------------------------------------------------------------------------------------
int RCCE_recv_pipe(char *privbuf, size_t size, int source) {
  int ignore;

#ifdef USE_PROBE_FLAGS
  RCCE_FLAG* probe = &RCCE_probe_flag[source];
#else
  RCCE_FLAG* probe = 0;
#endif

#ifndef USE_REMOTE_PUT_LOCAL_GET
  if(RCCE_recv_queue[source] != NULL)
#else
  if(RCCE_recv_queue != NULL)
#endif
    return(RCCE_REJECTED);

  ignore = 0;

#ifdef USE_PIPELINE_FLAGS
  return(RCCE_recv_general(privbuf, RCCE_buff_ptr, RCCE_chunk, 
			   &RCCE_ready_flag_pipe[RCCE_IAM], &RCCE_sent_flag_pipe[source], 
			   size, source, &ignore, 
			   1, 1, 0,          // copy, pipe, mcast
			   NULL, 0, probe)); // tag, len, probe
#else
  return(RCCE_recv_general(privbuf, RCCE_buff_ptr, RCCE_chunk, 
			   &RCCE_ready_flag[RCCE_IAM], &RCCE_sent_flag[source],
			   size, source, &ignore,
			   1, 1, 0,          // copy, pipe, mcast
			   NULL, 0, probe)); // tag, len, probe
#endif
}

int RCCE_recv_mcast(char *privbuf, size_t size, int source) {
  int ignore;

#ifdef USE_PROBE_FLAGS
  RCCE_FLAG* probe = &RCCE_probe_flag[source];
#else
  RCCE_FLAG* probe = 0;
#endif

#ifndef USE_REMOTE_PUT_LOCAL_GET
  if(RCCE_recv_queue[source] != NULL)
#else
  if(RCCE_recv_queue != NULL)
#endif
    return(RCCE_REJECTED);

  ignore = 0;
  return(RCCE_recv_general(privbuf, RCCE_buff_ptr, RCCE_chunk, 
			   NULL, NULL, 
			   size, source, &ignore,
			   1, 0, 1,          // copy, pipe, mcast
			   NULL, 0, probe)); // tag, len, probe
}

//--------------------------------------------------------------------------------------
// FUNCTION: RCCE_recv_cancel
//--------------------------------------------------------------------------------------
// recv function without copying the message into the recv buffer
//--------------------------------------------------------------------------------------
int RCCE_recv_cancel(size_t size, int source) {
  int ignore;

#ifdef USE_PROBE_FLAGS
  RCCE_FLAG* probe = &RCCE_probe_flag[source];
#else
  RCCE_FLAG* probe = 0;
#endif

#ifndef USE_REMOTE_PUT_LOCAL_GET
  if(RCCE_recv_queue[source] != NULL)
#else
  if(RCCE_recv_queue != NULL)
#endif
    return(RCCE_REJECTED);

  ignore = 0;
#ifdef USE_TAGGED_FOR_SHORT
  if(size <= (RCCE_LINE_SIZE - sizeof(int)))
  {
#ifdef USE_PROBE_FLAGS
    RCCE_wait_until(*probe, RCCE_FLAG_SET);
    RCCE_flag_write(probe, RCCE_FLAG_UNSET, RCCE_IAM);
#endif

#ifdef USE_REMOTE_PUT_LOCAL_GET

    RCCE_flag_write(&RCCE_ready_flag[RCCE_IAM], RCCE_FLAG_SET, source);
#ifndef USE_PROBE_FLAGS_SHORTCUT
    RCCE_wait_until(RCCE_sent_flag[source], RCCE_FLAG_SET);
    RCCE_flag_write(&RCCE_sent_flag[source], RCCE_FLAG_UNSET, RCCE_IAM);
#endif

#else // LOCAL PUT / REMOTE GET: (standard)

    RCCE_wait_until(RCCE_sent_flag[source], RCCE_FLAG_SET);
    RCCE_flag_write(&RCCE_sent_flag[source], RCCE_FLAG_UNSET, RCCE_IAM);
    RCCE_flag_write(&RCCE_ready_flag[RCCE_IAM], RCCE_FLAG_SET, source);  

#endif // !USE_REMOTE_PUT_LOCAL_GET

    return(RCCE_SUCCESS);
  }
  else
#endif
  return(RCCE_recv_general(NULL, RCCE_buff_ptr, RCCE_chunk, 
			   &RCCE_ready_flag[RCCE_IAM], &RCCE_sent_flag[source], 
			   size, source, &ignore,
			   0, 0, 0,          // copy, pipe, mcast
			   NULL, 0, probe)); // tag, len, probe
}

//--------------------------------------------------------------------------------------
// FUNCTION: RCCE_recv_test
//--------------------------------------------------------------------------------------
// recv_test function for simplified API; use library-maintained variables for 
// synchronization and set the test variable to 1 (do test)
//--------------------------------------------------------------------------------------
int RCCE_recv_test(char *privbuf, size_t size, int source, int *test) {

#ifdef USE_PROBE_FLAGS
  RCCE_FLAG* probe = &RCCE_probe_flag[source];
#else
  RCCE_FLAG* probe = 0;
#endif

#ifndef USE_REMOTE_PUT_LOCAL_GET
  if(RCCE_recv_queue[source] != NULL) {
#else
  if(RCCE_recv_queue != NULL) {
#endif
    (*test) = 0;
    return(RCCE_REJECTED);
  }

  
  /* make sure the test flag is set, regardless of input value */
  *test = 1;
  return(RCCE_recv_general(privbuf, RCCE_buff_ptr, RCCE_chunk, 
			   &RCCE_ready_flag[RCCE_IAM], &RCCE_sent_flag[source], 
			   size, source, test,
			   1, 0, 0,          // copy, pipe, mcast
			   NULL, 0, probe)); // tag, len, probe
}


//--------------------------------------------------------------------------------------
// FUNCTION: RCCE_recv_probe
//--------------------------------------------------------------------------------------
// probe for a message; just like RCCE_recv_test, but without any receiving
//--------------------------------------------------------------------------------------
int RCCE_recv_probe(int source, int *test, t_vcharp *combuf) {

#ifdef USE_PROBE_FLAGS
  RCCE_FLAG* flag = &RCCE_probe_flag[source];
#else
  RCCE_FLAG* flag = &RCCE_sent_flag[source];
#endif

#ifndef USE_REMOTE_PUT_LOCAL_GET
  if(RCCE_recv_queue[source] != NULL) {
#else
  if(RCCE_recv_queue != NULL) {
#endif
    (*test) = 0;
    (*combuf) = NULL;
    return(RCCE_REJECTED);
  }

  if(test) {
    RCCE_test_flag((*flag), RCCE_FLAG_SET, test);
#ifdef USE_REMOTE_PUT_LOCAL_GET
    if(combuf && (*test)) (*combuf) = RCCE_buff_ptr;
#else
    if(combuf && (*test)) (*combuf) = RCCE_comm_buffer[source]+(RCCE_buff_ptr-RCCE_comm_buffer[RCCE_IAM]);
#endif
  }
  else {
    RCCE_wait_until((*flag), RCCE_FLAG_SET);
#ifdef USE_REMOTE_PUT_LOCAL_GET
    if(combuf) (*combuf) = RCCE_buff_ptr;
#else
    if(combuf) (*combuf) = RCCE_comm_buffer[source]+(RCCE_buff_ptr-RCCE_comm_buffer[RCCE_IAM]);
#endif
  }

#ifdef USE_PROBE_FLAGS
  (*combuf) = NULL;
#endif
  
  return(RCCE_SUCCESS);
}

int RCCE_recv_probe_tagged(int source, int *test, t_vcharp *combuf, void* tag, int len) {

#ifdef USE_PROBE_FLAGS
  RCCE_FLAG* flag = &RCCE_probe_flag[source];
#else
  RCCE_FLAG* flag = &RCCE_sent_flag[source];
#endif

#ifndef USE_REMOTE_PUT_LOCAL_GET
  if(RCCE_recv_queue[source] != NULL) {
#else
  if(RCCE_recv_queue != NULL) {
#endif
    (*test) = 0;
    (*combuf) = NULL;
    return(RCCE_REJECTED);
  }

#ifdef USE_TAGGED_FLAGS
  if(test) {
    RCCE_test_tagged((*flag), RCCE_FLAG_SET, test, tag, len);
#ifdef USE_REMOTE_PUT_LOCAL_GET
    if(combuf && (*test)) (*combuf) = RCCE_buff_ptr;
#else
    if(combuf && (*test)) (*combuf) = RCCE_comm_buffer[source]+(RCCE_buff_ptr-RCCE_comm_buffer[RCCE_IAM]);
#endif
  }
  else {
    RCCE_wait_tagged((*flag), RCCE_FLAG_SET, tag, len);
#ifdef USE_REMOTE_PUT_LOCAL_GET
    if(combuf) (*combuf) = RCCE_buff_ptr;
#else
    if(combuf) (*combuf) = RCCE_comm_buffer[source]+(RCCE_buff_ptr-RCCE_comm_buffer[RCCE_IAM]);
#endif
  }
#else
  if(test) {
    RCCE_test_flag((*flag), RCCE_FLAG_SET, test);
  }
  else {
    RCCE_wait_until((*flag), RCCE_FLAG_SET);
  }

  if(!test || (test && (*test)))
  {
    RCCE_recv(tag, len, source);
    RCCE_wait_until((*flag), RCCE_FLAG_SET);
#ifdef USE_REMOTE_PUT_LOCAL_GET
    if(combuf) (*combuf) = RCCE_buff_ptr;
#else
    if(combuf) (*combuf) = RCCE_comm_buffer[source]+(RCCE_buff_ptr-RCCE_comm_buffer[RCCE_IAM]);
#endif
  } 
#endif

#ifdef USE_PROBE_FLAGS
  (*combuf) = NULL;
#endif

  return(RCCE_SUCCESS);
}

//--------------------------------------------------------------------------------------
// FUNCTION: RCCE_irecv
//--------------------------------------------------------------------------------------
// non-blocking recv function; returns an handle of type RCCE_RECV_REQUEST
//--------------------------------------------------------------------------------------
int RCCE_irecv(char *privbuf, size_t size, int source, RCCE_RECV_REQUEST *request) {

#ifdef USE_PROBE_FLAGS
  RCCE_FLAG* probe = &RCCE_probe_flag[source];
#else
  RCCE_FLAG* probe = 0;
#endif

  if(request == NULL){
    RCCE_RECV_REQUEST dummy_request;
    RCCE_irecv(privbuf, size, source, &dummy_request);
    RCCE_irecv_wait(&dummy_request);
    return(RCCE_SUCCESS);
  }

#ifdef USE_TAGGED_FOR_SHORT
  if(size <= (RCCE_LINE_SIZE - sizeof(int)))
    RCCE_init_recv_request(NULL, RCCE_buff_ptr, RCCE_chunk, 
			   &RCCE_ready_flag[RCCE_IAM], &RCCE_sent_flag[source], 
			   size, source, 0, privbuf, size, probe, request);
  else
#endif
  RCCE_init_recv_request(privbuf, RCCE_buff_ptr, RCCE_chunk, 
			 &RCCE_ready_flag[RCCE_IAM], &RCCE_sent_flag[source], 
			 size, source, 1, NULL, 0, probe, request);
  
#ifndef USE_REMOTE_PUT_LOCAL_GET
  if(RCCE_recv_queue[source] == NULL) {
#else
  if(RCCE_recv_queue == NULL) {
#endif
    
    if(RCCE_push_recv_request(request) == RCCE_SUCCESS) {
      return(RCCE_SUCCESS);
    }
    else {
#ifndef USE_REMOTE_PUT_LOCAL_GET
      RCCE_recv_queue[source] = request;
#else
      RCCE_recv_queue = request;
#endif
      return(RCCE_PENDING);
    }
  }
  else {
#ifndef USE_REMOTE_PUT_LOCAL_GET
    if(RCCE_recv_queue[source]->next == NULL) {
      RCCE_recv_queue[source]->next = request;
    }
#else
    if(RCCE_recv_queue->next == NULL) {
      RCCE_recv_queue->next = request;
    }
#endif
    else {
#ifndef USE_REMOTE_PUT_LOCAL_GET
      RCCE_RECV_REQUEST *run = RCCE_recv_queue[source];
#else
      RCCE_RECV_REQUEST *run = RCCE_recv_queue;
#endif
      while(run->next != NULL) run = run->next;      
      run->next = request;   
    }
    return(RCCE_RESERVED);
  }  
}

//--------------------------------------------------------------------------------------
// FUNCTION: RCCE_irecv_test
//--------------------------------------------------------------------------------------
// test function for completion of the requested non-blocking receive operation
//--------------------------------------------------------------------------------------
int RCCE_irecv_test(RCCE_RECV_REQUEST *request, int *test) {

  int source = request->source;

  if(request->finished) {
    (*test) = 1;
    return(RCCE_SUCCESS);
  }
  
#ifndef USE_REMOTE_PUT_LOCAL_GET
  if(RCCE_recv_queue[source] != request) {
#else
  if(RCCE_recv_queue != request) {
#endif
    (*test) = 0;
    return(RCCE_RESERVED);
  }

  RCCE_push_recv_request(request);
     
  if(request->finished) {
#ifndef USE_REMOTE_PUT_LOCAL_GET
    RCCE_recv_queue[source] = request->next;
#else
    RCCE_recv_queue = request->next;
#endif
   
    (*test) = 1;
    return(RCCE_SUCCESS);
  }

  (*test) = 0;
  return(RCCE_PENDING);
}


//--------------------------------------------------------------------------------------
// FUNCTION: RCCE_irecv_push
//--------------------------------------------------------------------------------------
// progress function for pending requests in the irecv queue 
//--------------------------------------------------------------------------------------
int RCCE_irecv_push(int source) {

#ifndef USE_REMOTE_PUT_LOCAL_GET
  RCCE_RECV_REQUEST *request = RCCE_recv_queue[source];
#else
  RCCE_RECV_REQUEST *request = RCCE_recv_queue;
#endif

  if(request == NULL) {
    return(RCCE_SUCCESS);
  }

  if(request->finished) {
    return(RCCE_SUCCESS);
  }
  
  RCCE_push_recv_request(request);   
     
  if(request->finished) {
#ifndef USE_REMOTE_PUT_LOCAL_GET
    RCCE_recv_queue[source] = request->next;
#else
    RCCE_recv_queue = request->next;
#endif
    return(RCCE_SUCCESS);
  }

  return(RCCE_PENDING);
}


//--------------------------------------------------------------------------------------
// FUNCTION: RCCE_irecv_wait
//--------------------------------------------------------------------------------------
// just wait for completion of the requestes non-blocking send operation
//--------------------------------------------------------------------------------------
int RCCE_irecv_wait(RCCE_RECV_REQUEST *request) {

  int ue;

#ifndef USE_REMOTE_PUT_LOCAL_GET
  while(!request->finished) {

    RCCE_irecv_push(request->source);

    if(!request->finished) {

      RCCE_isend_push(-1);

      for(ue=0; ue<RCCE_NP; ue++) {
	RCCE_irecv_push(ue);
      }
    }
  }
#else
  while(!request->finished) {

    RCCE_irecv_push(-1);

    if(!request->finished) {

      for(ue=0; ue<RCCE_NP; ue++) {
	RCCE_isend_push(ue);
      }

      RCCE_irecv_push(-1);
    }
  }
#endif
  
  return(RCCE_SUCCESS);
}

#else
// this is the gory synchronized message passing API      

//--------------------------------------------------------------------------------------
// FUNCTION: RCCE_recv
//--------------------------------------------------------------------------------------
// recv function for simplified API; use user-supplied variables for synchronization
// and set the test variable to 0 (ignore)
//--------------------------------------------------------------------------------------
int RCCE_recv(char *privbuf, t_vcharp combuf, size_t chunk, RCCE_FLAG *ready, 
              RCCE_FLAG *sent, size_t size, int source, RCCE_FLAG *probe) {
  int ignore = 0;
  return(RCCE_recv_general(privbuf, combuf, chunk, ready, sent, size, source,
                           &ignore,
			   1, 0, 0,   // copy, pipe, mcast
			   NULL, 0, probe)); // tag, len
}

//--------------------------------------------------------------------------------------
// FUNCTION: RCCE_recv_test
//--------------------------------------------------------------------------------------
// recv_test function for simplified API; use user-supplied variables for 
// synchronization and set the test variable to 1 (do test)
//--------------------------------------------------------------------------------------
int RCCE_recv_test(char *privbuf, t_vcharp combuf, size_t chunk, RCCE_FLAG *ready, 
              RCCE_FLAG *sent, size_t size, int source, int *test, RCCE_FLAG *probe) {
  /* make sure the test flag is set, regardless of input value */
  *test = 1;
  return(RCCE_recv_general(privbuf, combuf, chunk, ready, sent, size, source,
                           test,
			   1, 0, 0,   // copy, pipe, mcast
			   NULL, 0, probe)); // tag, len
}
#endif

