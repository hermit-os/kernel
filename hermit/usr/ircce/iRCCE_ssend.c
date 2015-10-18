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
//    [2010-11-26] added a _pipelined_ version of blocking send/recv
//                 by Carsten Clauss, Chair for Operating Systems,
//                                    RWTH Aachen University
//
//    [2011-05-31] added iRCCE_ANY_LENGTH wildcard mechanism
//                 by Carsten Clauss
//
//    [2011-11-03] added internal push function for non-blocking synchronous send
//                 iRCCE_push_ssend_request() (called by iRCCE_push_send_request)
//

#include "iRCCE_lib.h"
#include <stdlib.h>
#include <string.h>

#ifdef __hermit__
#include "rte_memcpy.h"
#define memcpy_scc rte_memcpy
#elif defined COPPERRIDGE || defined SCC
#include "scc_memcpy.h"
#else
#define memcpy_scc memcpy
#endif

//--------------------------------------------------------------------------------------
// FUNCTION: iRCCE_ssend_general
//--------------------------------------------------------------------------------------
// pipelined send function
//--------------------------------------------------------------------------------------
static int iRCCE_ssend_general(
		char *privbuf,    // source buffer in local private memory (send buffer)
		t_vcharp combuf,  // intermediate buffer in MPB
		size_t chunk,     // size of MPB available for this message (bytes)
		RCCE_FLAG *ready, // flag indicating whether receiver is ready
		RCCE_FLAG *sent,  // flag indicating whether message has been sent by source
		ssize_t size,     // size of message (bytes)
		int dest          // UE that will receive the message
		) {

	char padline[RCCE_LINE_SIZE]; // copy buffer, used if message not multiple of line size
	size_t wsize,                 // offset within send buffer when putting in "chunk" bytes
               remainder,             // bytes remaining to be sent
	       nbytes;                // number of bytes to be sent in single iRCCE_put call
	char *bufptr;                 // running pointer inside privbuf for current location
	size_t subchunk1, subchunk2;  // sub-chunks for the pipelined message transfer

#ifndef _iRCCE_ANY_LENGTH_
#define FLAG_SET_VALUE RCCE_FLAG_SET
#else
	RCCE_FLAG_STATUS FLAG_SET_VALUE = (RCCE_FLAG_STATUS)size;
#endif

	for (wsize = 0; wsize < (size/chunk)*chunk; wsize+=chunk) {

		if(wsize == 0) {
			// allign sub-chunks to cache line granularity:
			subchunk1 = ( (chunk / 2) / RCCE_LINE_SIZE ) * RCCE_LINE_SIZE;
			subchunk2 = chunk - subchunk1;
		}

		bufptr = privbuf + wsize;
		nbytes = subchunk1;
		  
		iRCCE_put(combuf, (t_vcharp) bufptr, nbytes, RCCE_IAM);

		RCCE_flag_write(sent, FLAG_SET_VALUE, dest);
		  
		RCCE_wait_until(*ready, RCCE_FLAG_SET);
		RCCE_flag_write(ready, RCCE_FLAG_UNSET, RCCE_IAM);      
		
		bufptr = privbuf + wsize + subchunk1;
		nbytes = subchunk2;
		
		iRCCE_put(combuf + subchunk1, (t_vcharp) bufptr, nbytes, RCCE_IAM);
		
		RCCE_flag_write(sent, FLAG_SET_VALUE, dest);
		
		RCCE_wait_until(*ready, RCCE_FLAG_SET);
		RCCE_flag_write(ready, RCCE_FLAG_UNSET, RCCE_IAM);      			
	}

	remainder = size%chunk; 
	// if nothing is left over, we are done 
	if (!remainder) return(iRCCE_SUCCESS);

	// send remainder of data--whole cache lines            
	bufptr = privbuf + (size/chunk)*chunk;
	nbytes = remainder - remainder%RCCE_LINE_SIZE;
	if (nbytes) {
		// copy private data to own comm buffer
		iRCCE_put(combuf, (t_vcharp)bufptr, nbytes, RCCE_IAM);
		RCCE_flag_write(sent, FLAG_SET_VALUE, dest);
		// wait for the destination to be ready to receive a message          
		RCCE_wait_until(*ready, RCCE_FLAG_SET);
		RCCE_flag_write(ready, RCCE_FLAG_UNSET, RCCE_IAM);
	}

	remainder = remainder%RCCE_LINE_SIZE;
	if (!remainder) return(iRCCE_SUCCESS);

	// remainder is less than a cache line. This must be copied into appropriately sized 
	// intermediate space before it can be sent to the receiver 
	bufptr = privbuf + (size/chunk)*chunk + nbytes;
	nbytes = RCCE_LINE_SIZE;

	// copy private data to own comm buffer 
	memcpy_scc(padline, bufptr, remainder);
	iRCCE_put(combuf, (t_vcharp)padline, nbytes, RCCE_IAM);
	RCCE_flag_write(sent, FLAG_SET_VALUE, dest);

	// wait for the destination to be ready to receive a message          
	RCCE_wait_until(*ready, RCCE_FLAG_SET);
	RCCE_flag_write(ready, RCCE_FLAG_UNSET, RCCE_IAM);

	return(iRCCE_SUCCESS);
}


//--------------------------------------------------------------------------------------
// FUNCTION: iRCCE_ssend
//--------------------------------------------------------------------------------------
// pipelined send function (blocking and synchronous!)
//--------------------------------------------------------------------------------------
int iRCCE_ssend(char *privbuf, ssize_t size, int dest) {

	if(size < 0) return(iRCCE_SUCCESS);

	if(size == 0) {
		// just synchronize:
		size = 1;
		privbuf = (char*)&size;
	}

	while(iRCCE_isend_queue != NULL) {

		// wait for completion of pending non-blocking requests
		iRCCE_isend_push();
		iRCCE_irecv_push();
	}

#if !defined(SINGLEBITFLAGS) && !defined(RCCE_VERSION)
	if(size <= iRCCE_MAX_TAGGED_LEN) {
		// just write the tagged 'sent' flag (with payload) and wait for 'ready' flag:
		iRCCE_flag_write_tagged(&RCCE_sent_flag[RCCE_IAM], (RCCE_FLAG_STATUS)size, dest, privbuf, size);
	  
		RCCE_wait_until(RCCE_ready_flag[dest], RCCE_FLAG_SET);
		RCCE_flag_write(&RCCE_ready_flag[dest], RCCE_FLAG_UNSET, RCCE_IAM);

		return(RCCE_SUCCESS);
	}
#endif

	if (dest<0 || dest >= RCCE_NP) 
		return(RCCE_error_return(RCCE_debug_comm,RCCE_ERROR_ID));
	else
		return(iRCCE_ssend_general(privbuf, RCCE_buff_ptr, RCCE_chunk, 
					  &RCCE_ready_flag[dest], &RCCE_sent_flag[RCCE_IAM], 
					   size, dest));
}


//--------------------------------------------------------------------------------------
// FUNCTION: iRCCE_push_ssend_request
//--------------------------------------------------------------------------------------
// pipelined push for send function (non-blocking and stricly synchronous!)
//--------------------------------------------------------------------------------------
int iRCCE_push_ssend_request(iRCCE_SEND_REQUEST *request) {

	char padline[RCCE_LINE_SIZE]; // copy buffer, used if message not multiple of line size
	int   test;       // flag for calling iRCCE_test_flag()

	if(request->finished) return(iRCCE_SUCCESS);

	if(request->label == 1) goto label1;
	if(request->label == 2) goto label2;
	if(request->label == 3) goto label3;
	if(request->label == 4) goto label4;

	// send data in units of available chunk size of comm buffer 
	for (request->wsize = 0; request->wsize < (request->size / request->chunk) * request->chunk; request->wsize += request->chunk) {

		request->bufptr = request->privbuf + request->wsize;
		request->nbytes = request->subchunk1;

		iRCCE_put(request->combuf, (t_vcharp) request->bufptr, request->nbytes, RCCE_IAM);
		RCCE_flag_write(request->sent, request->flag_set_value, request->dest);
label1:
		iRCCE_test_flag(*(request->ready), RCCE_FLAG_SET, &test);
		if(!test) {
			request->label = 1;
			return(iRCCE_PENDING);
		}
		RCCE_flag_write(request->ready, RCCE_FLAG_UNSET, RCCE_IAM);

		request->bufptr = request->privbuf + request->wsize + request->subchunk1;
		request->nbytes = request->subchunk2;
		
		iRCCE_put(request->combuf + request->subchunk1, (t_vcharp) request->bufptr, request->nbytes, RCCE_IAM);
		RCCE_flag_write(request->sent, request->flag_set_value, request->dest);
label2:
		iRCCE_test_flag(*(request->ready), RCCE_FLAG_SET, &test);
		if(!test) {
			request->label = 2;
			return(iRCCE_PENDING);
		}
		RCCE_flag_write(request->ready, RCCE_FLAG_UNSET, RCCE_IAM);		
	}

	request->remainder = request->size % request->chunk; 
	// if nothing is left over, we are done 
	if (!request->remainder) {
		request->finished = 1;
		return(iRCCE_SUCCESS);
	}

	// send remainder of data--whole cache lines            
	request->bufptr = request->privbuf + (request->size / request->chunk) * request->chunk;
	request->nbytes = request->remainder - request->remainder % RCCE_LINE_SIZE;
	if (request->nbytes) {
		// copy private data to own comm buffer
		iRCCE_put(request->combuf, (t_vcharp)request->bufptr, request->nbytes, RCCE_IAM);
		RCCE_flag_write(request->sent, request->flag_set_value, request->dest);
		// wait for the destination to be ready to receive a message          
label3:
		iRCCE_test_flag(*(request->ready), RCCE_FLAG_SET, &test);
		if(!test) {
			request->label = 3;
			return(iRCCE_PENDING);
		}
		RCCE_flag_write(request->ready, RCCE_FLAG_UNSET, RCCE_IAM);
	}

	request->remainder = request->size % request->chunk; 
	request->remainder = request->remainder%RCCE_LINE_SIZE;
	// if nothing is left over, we are done 
	if (!request->remainder)
	{
		request->finished = 1;
		return(iRCCE_SUCCESS);
	}

	// remainder is less than a cache line. This must be copied into appropriately sized 
	// intermediate space before it can be sent to the receiver 
	request->bufptr = request->privbuf + (request->size / request->chunk) * request->chunk + request->nbytes;
	request->nbytes = RCCE_LINE_SIZE;
	// copy private data to own comm buffer 
	memcpy(padline,request->bufptr,request->remainder);
	iRCCE_put(request->combuf, (t_vcharp)padline, request->nbytes, RCCE_IAM);
	RCCE_flag_write(request->sent, request->flag_set_value, request->dest);
	// wait for the destination to be ready to receive a message          
label4:
	iRCCE_test_flag(*(request->ready), RCCE_FLAG_SET, &test);
	if(!test) {
		request->label = 4;
		return(iRCCE_PENDING);
	}
	RCCE_flag_write(request->ready, RCCE_FLAG_UNSET, RCCE_IAM);

	request->finished = 1;
	return(iRCCE_SUCCESS);
}
