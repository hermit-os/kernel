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
// FUNCTION: RCCE_send_general
//--------------------------------------------------------------------------------------
// Synchronized send function (gory and non-gory mode)
//--------------------------------------------------------------------------------------
static int RCCE_send_general(
  char *privbuf,    // source buffer in local private memory (send buffer)
  t_vcharp combuf,  // intermediate buffer in MPB
  size_t chunk,     // size of MPB available for this message (bytes)
  RCCE_FLAG *ready, // flag indicating whether receiver is ready
  RCCE_FLAG *sent,  // flag indicating whether message has been sent by source
  size_t size,      // size of message (bytes)
  int dest,         // UE that will receive the message
  int copy,         // set to 0 for synchronization only (no copying/sending)
  int pipe,         // use pipelining?
  int mcast,        // multicast?
  void* tag,        // additional tag?
  int len,          // length of additional tag
  RCCE_FLAG *probe  // flag for probing for incoming messages
  ) {

  char padline[RCCE_LINE_SIZE]; // copy buffer, used if message not multiple of line size
  size_t wsize,    // offset within send buffer when putting in "chunk" bytes
        remainder, // bytes remaining to be sent
        nbytes;    // number of bytes to be sent in single RCCE_put call
  char *bufptr;    // running pointer inside privbuf for current location

#ifdef USE_REMOTE_PUT_LOCAL_GET
  if(mcast) return(RCCE_error_return(1, RCCE_ERROR_NO_MULTICAST_SUPPORT));
#endif

  if(probe)
#ifdef USE_TAGGED_FLAGS
    RCCE_flag_write_tagged(probe, RCCE_FLAG_SET, dest, tag, len);
#else
    RCCE_flag_write(probe, RCCE_FLAG_SET, dest);
#endif

#ifdef USE_SYNCH_FOR_ZERO_BYTE
  // synchronize even in case of zero byte messages:
  if(size == 0) {
#ifdef USE_REMOTE_PUT_LOCAL_GET
    RCCE_wait_until(*ready, RCCE_FLAG_SET);
    RCCE_flag_write(ready, RCCE_FLAG_UNSET, RCCE_IAM);
#ifdef USE_TAGGED_FLAGS
    if(!probe)
      RCCE_flag_write_tagged(sent, RCCE_FLAG_SET, dest, tag, len);
    else
#endif
    RCCE_flag_write(sent, RCCE_FLAG_SET, dest);
#else // LOCAL PUT / REMOTE GET: (standard)
#ifdef USE_TAGGED_FLAGS
    if(!probe)
      RCCE_flag_write_tagged(sent, RCCE_FLAG_SET, dest, tag, len);
    else
#endif
    RCCE_flag_write(sent, RCCE_FLAG_SET, dest);
    RCCE_wait_until(*ready, RCCE_FLAG_SET);
    RCCE_flag_write(ready, RCCE_FLAG_UNSET, RCCE_IAM);
#endif // !USE_REMOTE_PUT_LOCAL_GET
    return(RCCE_SUCCESS);
  }
#endif // USE_SYNCH_FOR_ZERO_BYTE

  if(!pipe) {
    // send data in units of available chunk size of comm buffer 
    for (wsize=0; wsize< (size/chunk)*chunk; wsize+=chunk) {
      bufptr = privbuf + wsize;
      nbytes = chunk;

#ifdef USE_REMOTE_PUT_LOCAL_GET

      RCCE_wait_until(*ready, RCCE_FLAG_SET);
      RCCE_flag_write(ready, RCCE_FLAG_UNSET, RCCE_IAM);
      
      // copy private data to remote comm buffer
      if(copy) RCCE_put(combuf, (t_vcharp) bufptr, nbytes, dest);

#ifdef USE_TAGGED_FLAGS
      if( (wsize == 0) && (!probe) )
	RCCE_flag_write_tagged(sent, RCCE_FLAG_SET, dest, tag, len);
      else
#endif
      RCCE_flag_write(sent, RCCE_FLAG_SET, dest);

#else // LOCAL PUT / REMOTE GET: (standard)

      // copy private data to own comm buffer
      if(copy) RCCE_put(combuf, (t_vcharp) bufptr, nbytes, RCCE_IAM);

      if(!mcast) {
#ifdef USE_TAGGED_FLAGS
	if( (wsize == 0) && (!probe) )
	  RCCE_flag_write_tagged(sent, RCCE_FLAG_SET, dest, tag, len);
	else
#endif
	RCCE_flag_write(sent, RCCE_FLAG_SET, dest);

	// wait for the destination to be ready to receive a message          
	RCCE_wait_until(*ready, RCCE_FLAG_SET);
	RCCE_flag_write(ready, RCCE_FLAG_UNSET, RCCE_IAM);
      }
      else {
	RCCE_TNS_barrier(&RCCE_COMM_WORLD);
	RCCE_TNS_barrier(&RCCE_COMM_WORLD);
      }
#endif // !USE_REMOTE_PUT_LOCAL_GET

    } // for
  }
  else // if(!pipe) ->  if(pipe)
  {
    // pipelined version of send/recv:
    size_t subchunk1, subchunk2;

    for(wsize = 0; wsize < (size/chunk)*chunk; wsize+=chunk) {

      if(wsize == 0) {
	// allign sub-chunks to cache line granularity:
	subchunk1 = ( (chunk / 2) / RCCE_LINE_SIZE ) * RCCE_LINE_SIZE;
	subchunk2 = chunk - subchunk1;
      }

      bufptr = privbuf + wsize;
      nbytes = subchunk1;

#ifdef USE_REMOTE_PUT_LOCAL_GET

      RCCE_wait_until(*ready, RCCE_FLAG_SET);
      RCCE_flag_write(ready, RCCE_FLAG_UNSET, RCCE_IAM);

      // copy private data chunk 1 to remote comm buffer
      if(copy) RCCE_put(combuf, (t_vcharp) bufptr, nbytes, dest);

#ifdef USE_TAGGED_FLAGS
      if( (wsize == 0) && (!probe) )
	RCCE_flag_write_tagged(sent, RCCE_FLAG_SET, dest, tag, len);
      else
#endif
      RCCE_flag_write(sent, RCCE_FLAG_SET, dest);

#else // LOCAL PUT / REMOTE GET: (standard)
      
      // copy private data chunk 1 to own comm buffer
      if(copy) RCCE_put(combuf, (t_vcharp) bufptr, nbytes, RCCE_IAM);

#ifdef USE_TAGGED_FLAGS
      if( (wsize == 0) && (!probe) )
	RCCE_flag_write_tagged(sent, RCCE_FLAG_SET, dest, tag, len);
      else
#endif
      RCCE_flag_write(sent, RCCE_FLAG_SET, dest);
      
      RCCE_wait_until(*ready, RCCE_FLAG_SET);
      RCCE_flag_write(ready, RCCE_FLAG_UNSET, RCCE_IAM);

#endif // !USE_REMOTE_PUT_LOCAL_GET      
      
      bufptr = privbuf + wsize + subchunk1;
      nbytes = subchunk2;
      
#ifdef USE_REMOTE_PUT_LOCAL_GET

      RCCE_wait_until(*ready, RCCE_FLAG_SET);
      RCCE_flag_write(ready, RCCE_FLAG_UNSET, RCCE_IAM);
      
      // copy private data chunk 2 to remote comm buffer
      if(copy) RCCE_put(combuf + subchunk1, (t_vcharp) bufptr, nbytes, dest);

      RCCE_flag_write(sent, RCCE_FLAG_SET, dest);

#else // LOCAL PUT / REMOTE GET: (standard)

      // copy private data chunk 2 to own comm buffer
      if(copy) RCCE_put(combuf + subchunk1, (t_vcharp) bufptr, nbytes, RCCE_IAM);
      
      RCCE_flag_write(sent, RCCE_FLAG_SET, dest);
      
      RCCE_wait_until(*ready, RCCE_FLAG_SET);
      RCCE_flag_write(ready, RCCE_FLAG_UNSET, RCCE_IAM);

#endif // !USE_REMOTE_PUT_LOCAL_GET

    } //for

  } // if(pipe)

  remainder = size%chunk; 
  // if nothing is left over, we are done 
  if (!remainder) return(RCCE_SUCCESS);

  // send remainder of data--whole cache lines            
  bufptr = privbuf + (size/chunk)*chunk;
  nbytes = remainder - remainder%RCCE_LINE_SIZE;

  if (nbytes) {

#ifdef USE_REMOTE_PUT_LOCAL_GET

    RCCE_wait_until(*ready, RCCE_FLAG_SET);
    RCCE_flag_write(ready, RCCE_FLAG_UNSET, RCCE_IAM);
    
    // copy private data to remote comm buffer
    if(copy) RCCE_put(combuf, (t_vcharp) bufptr, nbytes, dest);

#ifdef USE_TAGGED_FLAGS
    if( (wsize == 0) && (!probe) )
      RCCE_flag_write_tagged(sent, RCCE_FLAG_SET, dest, tag, len);
    else
#endif
    RCCE_flag_write(sent, RCCE_FLAG_SET, dest);
    
#else // LOCAL PUT / REMOTE GET: (standard)

    // copy private data to own comm buffer
    if(copy) RCCE_put(combuf, (t_vcharp)bufptr, nbytes, RCCE_IAM);

    if(!mcast) {
#ifdef USE_TAGGED_FLAGS
      if( (wsize == 0) && (!probe) )
	RCCE_flag_write_tagged(sent, RCCE_FLAG_SET, dest, tag, len);
      else
#endif
      RCCE_flag_write(sent, RCCE_FLAG_SET, dest);

      // wait for the destination to be ready to receive a message          
      RCCE_wait_until(*ready, RCCE_FLAG_SET);
      RCCE_flag_write(ready, RCCE_FLAG_UNSET, RCCE_IAM);
    }
    else {
      RCCE_TNS_barrier(&RCCE_COMM_WORLD);
      RCCE_TNS_barrier(&RCCE_COMM_WORLD);
    }
#endif // !USE_REMOTE_PUT_LOCAL_GET

  } // if(nbytes)
   
  remainder = remainder%RCCE_LINE_SIZE;
  if (!remainder) return(RCCE_SUCCESS);
  
  // remainder is less than a cache line. This must be copied into appropriately sized 
  // intermediate space before it can be sent to the receiver 
  bufptr = privbuf + (size/chunk)*chunk + nbytes;
  nbytes = RCCE_LINE_SIZE;

  if(copy) {
#ifdef COPPERRIDGE
    memcpy_scc(padline,bufptr,remainder);
#else
    memcpy(padline,bufptr,remainder);
#endif
  }

#ifdef USE_REMOTE_PUT_LOCAL_GET

  RCCE_wait_until(*ready, RCCE_FLAG_SET);
  RCCE_flag_write(ready, RCCE_FLAG_UNSET, RCCE_IAM);
  
  // copy private data to remote comm buffer
  if(copy) RCCE_put(combuf, (t_vcharp) padline, nbytes, dest);

#ifdef USE_TAGGED_FLAGS
  if( (wsize == 0) && (!probe) )
    RCCE_flag_write_tagged(sent, RCCE_FLAG_SET, dest, tag, len);
  else
#endif
  RCCE_flag_write(sent, RCCE_FLAG_SET, dest);

#else // LOCAL PUT / REMOTE GET: (standard)

  // copy private data to own comm buffer 
  if(copy) RCCE_put(combuf, (t_vcharp)padline, nbytes, RCCE_IAM);
  
  if(!mcast) {
#ifdef USE_TAGGED_FLAGS
    if( (wsize == 0) && (!probe) )
      RCCE_flag_write_tagged(sent, RCCE_FLAG_SET, dest, tag, len);
    else
#endif
    RCCE_flag_write(sent, RCCE_FLAG_SET, dest);

    // wait for the destination to be ready to receive a message          
    RCCE_wait_until(*ready, RCCE_FLAG_SET);
    RCCE_flag_write(ready, RCCE_FLAG_UNSET, RCCE_IAM);
  }
  else {
    RCCE_TNS_barrier(&RCCE_COMM_WORLD);
    RCCE_TNS_barrier(&RCCE_COMM_WORLD);
  }

#endif // !USE_REMOTE_PUT_LOCAL_GET

  return(RCCE_SUCCESS);
}

static int RCCE_push_send_request(RCCE_SEND_REQUEST *request) {

  char padline[RCCE_LINE_SIZE]; // copy buffer, used if message not multiple of line size
  int   test;       // flag for calling RCCE_test_flag()

  if(request->finished) return(RCCE_SUCCESS);

  if(request->label == 1) goto label1;
  if(request->label == 2) goto label2;
  if(request->label == 3) goto label3;
  if(request->label == 4) goto label4;

  if(request->probe)
#ifdef USE_TAGGED_FLAGS
    RCCE_flag_write_tagged(request->probe, RCCE_FLAG_SET, request->dest, request->tag, request->len);
#else
    RCCE_flag_write(request->probe, RCCE_FLAG_SET, request->dest);
#endif

#ifdef USE_SYNCH_FOR_ZERO_BYTE
  // synchronize even in case of zero byte messages:
  if(request->size == 0) {
#ifdef USE_REMOTE_PUT_LOCAL_GET
  label1:
    RCCE_test_flag(*(request->ready), RCCE_FLAG_SET, &test);
    if(!test) {
      request->label = 1;
      return(RCCE_PENDING);
    }
    RCCE_flag_write(request->ready, RCCE_FLAG_UNSET, RCCE_IAM);
#ifdef USE_TAGGED_FLAGS
    if(!request->probe)
      RCCE_flag_write_tagged(request->sent, RCCE_FLAG_SET, request->dest, request->tag, request->len);
    else
#endif
    RCCE_flag_write(request->sent, RCCE_FLAG_SET, request->dest);
#else // LOCAL PUT / REMOTE GET: (standard)
#ifdef USE_TAGGED_FLAGS
    if(!request->probe)
      RCCE_flag_write_tagged(request->sent, RCCE_FLAG_SET, request->dest, request->tag, request->len);
    else
#endif
    RCCE_flag_write(request->sent, RCCE_FLAG_SET, request->dest);
  label1:
    RCCE_test_flag(*(request->ready), RCCE_FLAG_SET, &test);
    if(!test) {
      request->label = 1;
      return(RCCE_PENDING);
    }
    RCCE_flag_write(request->ready, RCCE_FLAG_UNSET, RCCE_IAM);
#endif // !USE_REMOTE_PUT_LOCAL_GET
    request->finished = 1;
    return(RCCE_SUCCESS);
  }
#endif // USE_SYNCH_FOR_ZERO_BYTE

  // send data in units of available chunk size of comm buffer 
  for (; request->wsize < (request->size / request->chunk) * request->chunk; request->wsize += request->chunk) {
    request->bufptr = request->privbuf + request->wsize;
    request->nbytes = request->chunk;

#ifdef USE_REMOTE_PUT_LOCAL_GET

    // wait for the destination to be ready to receive a message
  label2:
    RCCE_test_flag(*(request->ready), RCCE_FLAG_SET, &test);
    if(!test) {
      request->label = 2;
      return(RCCE_PENDING);
    }
    RCCE_flag_write(request->ready, RCCE_FLAG_UNSET, RCCE_IAM);

    // copy private data to remote comm buffer
    if(request->copy) RCCE_put(request->combuf, (t_vcharp) request->bufptr, request->nbytes, request->dest);

#ifdef USE_TAGGED_FLAGS
    if( (request->wsize == 0) && (!request->probe) )
      RCCE_flag_write_tagged(request->sent, RCCE_FLAG_SET, request->dest, request->tag, request->len);
    else
#endif
    RCCE_flag_write(request->sent, RCCE_FLAG_SET, request->dest);

#else // LOCAL PUT / REMOTE GET: (standard)

    // copy private data to own comm buffer
    if(request->copy) RCCE_put(request->combuf, (t_vcharp) request->bufptr, request->nbytes, RCCE_IAM);
    
#ifdef USE_TAGGED_FLAGS
    if( (request->wsize == 0) && (!request->probe) )
      RCCE_flag_write_tagged(request->sent, RCCE_FLAG_SET, request->dest, request->tag, request->len);
    else
#endif
    RCCE_flag_write(request->sent, RCCE_FLAG_SET, request->dest);

    // wait for the destination to be ready to receive a message          
  label2:
    RCCE_test_flag(*(request->ready), RCCE_FLAG_SET, &test);
    if(!test) {
      request->label = 2;
      return(RCCE_PENDING);
    }
    RCCE_flag_write(request->ready, RCCE_FLAG_UNSET, RCCE_IAM);

#endif // !USE_REMOTE_PUT_LOCAL_GET

  } // for

  request->remainder = request->size % request->chunk; 
  // if nothing is left over, we are done 
  if (!request->remainder) {
    request->finished = 1;
    return(RCCE_SUCCESS);
  }

  // send remainder of data--whole cache lines            
  request->bufptr = request->privbuf + (request->size / request->chunk) * request->chunk;
  request->nbytes = request->remainder - request->remainder % RCCE_LINE_SIZE;

  if (request->nbytes) {

#ifdef USE_REMOTE_PUT_LOCAL_GET

    // wait for the destination to be ready to receive a message
  label3:
    RCCE_test_flag(*(request->ready), RCCE_FLAG_SET, &test);
    if(!test) {
      request->label = 3;
      return(RCCE_PENDING);
    }
    RCCE_flag_write(request->ready, RCCE_FLAG_UNSET, RCCE_IAM);

    // copy private data to remote comm buffer
    if(request->copy) RCCE_put(request->combuf, (t_vcharp) request->bufptr, request->nbytes, request->dest);

#ifdef USE_TAGGED_FLAGS
    if( (request->wsize == 0) && (!request->probe) )
      RCCE_flag_write_tagged(request->sent, RCCE_FLAG_SET, request->dest, request->tag, request->len);
    else
#endif
    RCCE_flag_write(request->sent, RCCE_FLAG_SET, request->dest);

#else // LOCAL PUT / REMOTE GET: (standard)

    // copy private data to own comm buffer
    if(request->copy) RCCE_put(request->combuf, (t_vcharp)request->bufptr, request->nbytes, RCCE_IAM);

#ifdef USE_TAGGED_FLAGS
    if( (request->wsize == 0) && (!request->probe) )
      RCCE_flag_write_tagged(request->sent, RCCE_FLAG_SET, request->dest, request->tag, request->len);
    else
#endif
    RCCE_flag_write(request->sent, RCCE_FLAG_SET, request->dest);

    // wait for the destination to be ready to receive a message          
  label3:
    RCCE_test_flag(*(request->ready), RCCE_FLAG_SET, &test);
    if(!test) {
      request->label = 3;
      return(RCCE_PENDING);
    }
    RCCE_flag_write(request->ready, RCCE_FLAG_UNSET, RCCE_IAM);

#endif // !USE_REMOTE_PUT_LOCAL_GET

  } //  if(request->nbytes)

  request->remainder = request->size % request->chunk; 
  request->remainder = request->remainder%RCCE_LINE_SIZE;

  // if nothing is left over, we are done 
  if (!request->remainder)
  {
    request->finished = 1;
    return(RCCE_SUCCESS);
  }
  
  // remainder is less than a cache line. This must be copied into appropriately sized 
  // intermediate space before it can be sent to the receiver 
  request->bufptr = request->privbuf + (request->size / request->chunk) * request->chunk + request->nbytes;
  request->nbytes = RCCE_LINE_SIZE;

#ifdef USE_REMOTE_PUT_LOCAL_GET

  // wait for the destination to be ready to receive a message
 label4:
  RCCE_test_flag(*(request->ready), RCCE_FLAG_SET, &test);
  if(!test) {
    request->label = 4;
    return(RCCE_PENDING);
  }
  RCCE_flag_write(request->ready, RCCE_FLAG_UNSET, RCCE_IAM);
  
  // copy private data to remote comm buffer
  if(request->copy) {
#ifdef COPPERRIDGE
    memcpy_scc(padline,request->bufptr,request->remainder);
#else
    memcpy(padline,request->bufptr,request->remainder);
#endif 
    RCCE_put(request->combuf, (t_vcharp) padline, request->nbytes, request->dest);
  }

#ifdef USE_TAGGED_FLAGS
#ifdef USE_PROBE_FLAGS_SHORTCUT
  if(request->privbuf == NULL) 
  {
    request->finished = 1;
    return(RCCE_SUCCESS);
  }
#endif
  if( (request->wsize == 0) && (!request->probe) )
    RCCE_flag_write_tagged(request->sent, RCCE_FLAG_SET, request->dest, request->tag, request->len);
  else
#endif
  RCCE_flag_write(request->sent, RCCE_FLAG_SET, request->dest);

#else // LOCAL PUT / REMOTE GET: (standard)
  
  // copy private data to own comm buffer 
  if(request->copy) {
#ifdef COPPERRIDGE
    memcpy_scc(padline,request->bufptr,request->remainder);
#else
    memcpy(padline,request->bufptr,request->remainder);
#endif
    RCCE_put(request->combuf, (t_vcharp)padline, request->nbytes, RCCE_IAM);
  }

#ifdef USE_TAGGED_FLAGS
  if( (request->wsize == 0) && (!request->probe) )
    RCCE_flag_write_tagged(request->sent, RCCE_FLAG_SET, request->dest, request->tag, request->len);
  else
#endif
  RCCE_flag_write(request->sent, RCCE_FLAG_SET, request->dest);

  // wait for the destination to be ready to receive a message          
 label4:
  RCCE_test_flag(*(request->ready), RCCE_FLAG_SET, &test);
  if(!test) {
      request->label = 4;
      return(RCCE_PENDING);
  }
  RCCE_flag_write(request->ready, RCCE_FLAG_UNSET, RCCE_IAM);

#endif // !USE_REMOTE_PUT_LOCAL_GET

  request->finished = 1;
  return(RCCE_SUCCESS);
}

static void RCCE_init_send_request(
  char *privbuf,    // source buffer in local private memory (send buffer)
  t_vcharp combuf,  // intermediate buffer in MPB
  size_t chunk,     // size of MPB available for this message (bytes)
  RCCE_FLAG *ready, // flag indicating whether receiver is ready
  RCCE_FLAG *sent,  // flag indicating whether message has been sent by source
  size_t size,      // size of message (bytes)
  int dest,         // UE that will receive the message
  int copy,         // set to 0 for synchronization only (no copying/sending)
  void* tag,        // additional tag?
  int len,          // length of additional tag
  RCCE_FLAG *probe, // flag for probing for incoming messages
  RCCE_SEND_REQUEST *request
  ) {

  request->privbuf   = privbuf;
  request->combuf    = combuf;
  request->chunk     = chunk;
  request->ready     = ready;
  request->sent      = sent;
  request->size      = size;
  request->dest      = dest;

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
// this is the LfBS-customized synchronized message passing API      

//--------------------------------------------------------------------------------------
// FUNCTION: RCCE_send
//--------------------------------------------------------------------------------------
// send function for simplified API; use library-maintained variables for synchronization
//--------------------------------------------------------------------------------------
int RCCE_send(char *privbuf, size_t size, int dest) {

#ifdef USE_PROBE_FLAGS
  RCCE_FLAG* probe = &RCCE_probe_flag[RCCE_IAM];
#else
  RCCE_FLAG* probe = NULL;
#endif

#ifndef USE_REMOTE_PUT_LOCAL_GET
  if(RCCE_send_queue != NULL)
#else
  if(RCCE_send_queue[dest] != NULL)
#endif
     return(RCCE_REJECTED);

#ifdef USE_TAGGED_FOR_SHORT
  if(size <= (RCCE_LINE_SIZE - sizeof(int)))
  {
#ifdef USE_PROBE_FLAGS
    RCCE_flag_write_tagged(probe, RCCE_FLAG_SET, dest, privbuf, size);
#endif

#ifdef USE_REMOTE_PUT_LOCAL_GET

    RCCE_wait_until(RCCE_ready_flag[dest], RCCE_FLAG_SET);
    RCCE_flag_write(&RCCE_ready_flag[dest], RCCE_FLAG_UNSET, RCCE_IAM);

#ifndef USE_PROBE_FLAGS_SHORTCUT
#ifdef USE_PROBE_FLAGS
    RCCE_flag_write(&RCCE_sent_flag[RCCE_IAM], RCCE_FLAG_SET, dest);
#else
    RCCE_flag_write_tagged(&RCCE_sent_flag[RCCE_IAM], RCCE_FLAG_SET, dest, privbuf, size);
#endif
#endif

#else // LOCAL PUT / REMOTE GET: (standard)
  
#ifdef USE_PROBE_FLAGS
    RCCE_flag_write(&RCCE_sent_flag[RCCE_IAM], RCCE_FLAG_SET, dest);
#else
    RCCE_flag_write_tagged(&RCCE_sent_flag[RCCE_IAM], RCCE_FLAG_SET, dest, privbuf, size);
#endif

    RCCE_wait_until(RCCE_ready_flag[dest], RCCE_FLAG_SET);
    RCCE_flag_write(&RCCE_ready_flag[dest], RCCE_FLAG_UNSET, RCCE_IAM);

#endif // !USE_REMOTE_PUT_LOCAL_GET

    return(RCCE_SUCCESS);
  }
  else
#endif

  return(RCCE_send_general(privbuf, RCCE_buff_ptr, RCCE_chunk, 
			   &RCCE_ready_flag[dest], &RCCE_sent_flag[RCCE_IAM],
			   size, dest,
			   1, 0, 0,          // copy, pipe, mcast
			   NULL, 0, probe)); // tag, len
}

int RCCE_send_tagged(char *privbuf, size_t size, int dest, void* tag, int len) {

#ifdef USE_PROBE_FLAGS
  RCCE_FLAG* probe = &RCCE_probe_flag[RCCE_IAM];
#else
  RCCE_FLAG* probe = NULL;
#endif

#ifndef USE_REMOTE_PUT_LOCAL_GET
  if(RCCE_send_queue != NULL)
#else
  if(RCCE_send_queue[dest] != NULL)
#endif
     return(RCCE_REJECTED);

#ifdef USE_TAGGED_FLAGS
  return(RCCE_send_general(privbuf, RCCE_buff_ptr, RCCE_chunk, 
			   &RCCE_ready_flag[dest], &RCCE_sent_flag[RCCE_IAM],
			   size, dest,
			   1, 0, 0,           // copy, pipe, mcast
			   tag, len, probe)); // tag, len, probe
#else

  RCCE_send_general(tag, RCCE_buff_ptr, RCCE_chunk, 
		    &RCCE_ready_flag[dest], &RCCE_sent_flag[RCCE_IAM],
		    len, dest,
		    1, 0, 0,         // copy, pipe, mcast
		    NULL, 0, probe); // tag, len, probe

  return(RCCE_send_general(privbuf, RCCE_buff_ptr, RCCE_chunk, 
			   &RCCE_ready_flag[dest], &RCCE_sent_flag[RCCE_IAM],
			   size, dest,
			   1, 0, 0,         // copy, pipe, mcast
			   NULL, 0, NULL)); // tag, len, probe
#endif  
}

//--------------------------------------------------------------------------------------
// FUNCTION: RCCE_send_pipe
//--------------------------------------------------------------------------------------
// send function for simplified API; use library-maintained variables for synchronization
//--------------------------------------------------------------------------------------
int RCCE_send_pipe(char *privbuf, size_t size, int dest) {

#ifdef USE_PROBE_FLAGS
  RCCE_FLAG* probe = &RCCE_probe_flag[RCCE_IAM];
#else
  RCCE_FLAG* probe = NULL;
#endif

#ifndef USE_REMOTE_PUT_LOCAL_GET
  if(RCCE_send_queue != NULL)
#else
  if(RCCE_send_queue[dest] != NULL)
#endif
     return(RCCE_REJECTED);

#ifdef USE_PIPELINE_FLAGS
  return(RCCE_send_general(privbuf, RCCE_buff_ptr, RCCE_chunk, 
			   &RCCE_ready_flag_pipe[dest], &RCCE_sent_flag_pipe[RCCE_IAM], 
			   size, dest,
			   1, 1, 0,          // copy, pipe, mcast
			   NULL, 0, probe)); // tag, len, probe
#else
  return(RCCE_send_general(privbuf, RCCE_buff_ptr, RCCE_chunk, 
			   &RCCE_ready_flag[dest], &RCCE_sent_flag[RCCE_IAM],
			   size, dest,
			   1, 1, 0,          // copy, pipe, mcast
			   NULL, 0, probe)); // tag, len, probe
#endif
}

int RCCE_send_mcast(char *privbuf, size_t size) {

#ifdef USE_PROBE_FLAGS
  RCCE_FLAG* probe = &RCCE_probe_flag[RCCE_IAM];
#else
  RCCE_FLAG* probe = NULL;
#endif

#ifndef USE_REMOTE_PUT_LOCAL_GET
  if(RCCE_send_queue != NULL)
#else
  if(RCCE_send_queue != NULL)
#endif
     return(RCCE_REJECTED);

  return(RCCE_send_general(privbuf, RCCE_buff_ptr, RCCE_chunk, 
			   NULL, NULL, 
			   size, -1,
			   1, 0, 1,          // copy, pipe, mcast
			   NULL, 0, probe)); // tag, len
}

//--------------------------------------------------------------------------------------
// FUNCTION: RCCE_isend
//--------------------------------------------------------------------------------------
// non-blocking send function; returns an handle of type RCCE_SEND_REQUEST
//--------------------------------------------------------------------------------------
int RCCE_isend(char *privbuf, size_t size, int dest, RCCE_SEND_REQUEST *request) {

#ifdef USE_PROBE_FLAGS
  RCCE_FLAG* probe = &RCCE_probe_flag[RCCE_IAM];
#else
  RCCE_FLAG* probe = NULL;
#endif

#ifdef USE_TAGGED_FOR_SHORT
  if(size <= (RCCE_LINE_SIZE - sizeof(int)))
  {
    RCCE_init_send_request(NULL, RCCE_buff_ptr, RCCE_chunk, 
			   &RCCE_ready_flag[dest], &RCCE_sent_flag[RCCE_IAM], 
			   size, dest, 0, privbuf, size, probe, request);
  }
  else
#endif

  RCCE_init_send_request(privbuf, RCCE_buff_ptr, RCCE_chunk, 
			 &RCCE_ready_flag[dest], &RCCE_sent_flag[RCCE_IAM], 
			 size, dest, 1, NULL, 0, probe, request);
  
#ifndef USE_REMOTE_PUT_LOCAL_GET
  if(RCCE_send_queue == NULL) {
#else
  if(RCCE_send_queue[dest] == NULL) {
#endif
    
    if(RCCE_push_send_request(request) == RCCE_SUCCESS) {
      return(RCCE_SUCCESS);
    }
    else {
#ifndef USE_REMOTE_PUT_LOCAL_GET
      RCCE_send_queue = request;
#else
      RCCE_send_queue[dest] = request;
#endif
      return(RCCE_PENDING);
    }
  }
  else {
#ifndef USE_REMOTE_PUT_LOCAL_GET
    if(RCCE_send_queue->next == NULL) {
      RCCE_send_queue->next = request;
    }
#else
    if(RCCE_send_queue[dest]->next == NULL) {
      RCCE_send_queue[dest]->next = request;
    }
#endif    
    else {
#ifndef USE_REMOTE_PUT_LOCAL_GET
      RCCE_SEND_REQUEST *run = RCCE_send_queue;
#else
      RCCE_SEND_REQUEST *run = RCCE_send_queue[dest];
#endif
      while(run->next != NULL) run = run->next;      
      run->next = request;   
    }
    return(RCCE_RESERVED);
  }  
}

//--------------------------------------------------------------------------------------
// FUNCTION: RCCE_isend_test
//--------------------------------------------------------------------------------------
// test function for completion of the requestes non-blocking send operation
//--------------------------------------------------------------------------------------
int RCCE_isend_test(RCCE_SEND_REQUEST *request, int *test) {

  if(request->finished) {
    (*test) = 1;
    return(RCCE_SUCCESS);
  }
  
#ifndef USE_REMOTE_PUT_LOCAL_GET
  if(RCCE_send_queue != request) {
#else
  if(RCCE_send_queue[request->dest] != request) {
#endif
    (*test) = 0;
    return(RCCE_RESERVED);
  }

  RCCE_push_send_request(request);   
     
  if(request->finished) {
#ifndef USE_REMOTE_PUT_LOCAL_GET
    RCCE_send_queue = request->next;
#else
    RCCE_send_queue[request->dest] = request->next;
#endif
   
    (*test) = 1;
    return(RCCE_SUCCESS);
  }

  (*test) = 0;
  return(RCCE_PENDING);
}


//--------------------------------------------------------------------------------------
// FUNCTION: RCCE_isend_push
//--------------------------------------------------------------------------------------
// progress function for pending requests in the isend queue 
//--------------------------------------------------------------------------------------
int RCCE_isend_push(int dest) {

#ifndef USE_REMOTE_PUT_LOCAL_GET
  RCCE_SEND_REQUEST *request = RCCE_send_queue;
#else
  RCCE_SEND_REQUEST *request = RCCE_send_queue[dest];
#endif

  if(request == NULL) {
    return(RCCE_SUCCESS);
  }

  if(request->finished) {
    return(RCCE_SUCCESS);
  }
  
  RCCE_push_send_request(request);   
     
  if(request->finished) {
#ifndef USE_REMOTE_PUT_LOCAL_GET
    RCCE_send_queue = request->next;
#else
    RCCE_send_queue[request->dest] = request->next;
#endif
    return(RCCE_SUCCESS);
  }

  return(RCCE_PENDING);
}

//--------------------------------------------------------------------------------------
// FUNCTION: RCCE_isend_wait
//--------------------------------------------------------------------------------------
// just wait for completion of the requested non-blocking send operation
//--------------------------------------------------------------------------------------
int RCCE_isend_wait(RCCE_SEND_REQUEST *request) {

  int ue;

#ifndef USE_REMOTE_PUT_LOCAL_GET
  while(!request->finished) {

    RCCE_isend_push(-1);

    if(!request->finished) {

      for(ue=0; ue<RCCE_NP; ue++) {
	RCCE_irecv_push(ue);
      }
    }
  }
#else
  while(!request->finished) {

    RCCE_isend_push(request->dest);

    if(!request->finished) {

      RCCE_irecv_push(-1);

      for(ue=0; ue<RCCE_NP; ue++) {
	RCCE_isend_push(ue);
      }
    }
  }
#endif
  
  return(RCCE_SUCCESS);
}

#else
// this is the gory synchronized message passing API      

//--------------------------------------------------------------------------------------
// FUNCTION: RCCE_send
//--------------------------------------------------------------------------------------
// send function for simplified API; use user-supplied variables for synchronization
//--------------------------------------------------------------------------------------
int RCCE_send(char *privbuf, t_vcharp combuf, size_t chunk, RCCE_FLAG *ready, 
              RCCE_FLAG *sent, size_t size, int dest) {
  return(RCCE_send_general(privbuf, combuf, chunk, ready, sent,
			   size, dest,
			   1, 0, 0,         // copy, pipe, mcast
			   NULL, 0, NULL)); // tag, len, probe
}
#endif
