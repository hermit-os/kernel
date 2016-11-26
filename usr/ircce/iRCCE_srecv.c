//***************************************************************************************
// Non-blocking receive routines. 
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
//    [2011-04-19] added wildcard mechanism (iRCCE_ANY_SOURCE) for receiving
//                 a message from an arbitrary remote rank
//                 by Simon Pickartz, Chair for Operating Systems,
//                                    RWTH Aachen University
//
//    [2011-05-31] added iRCCE_ANY_LENGTH wildcard mechanism
//                 by Carsten Clauss
//
//    [2011-06-27] merged iRCCE_ANY_SOURCE branch with trunk (iRCCE_ANY_LENGTH)
//
//    [2011-08-02] added iRCCE_iprobe() function for probing for incomming messages
//
//    [2011-11-03] added internal push function for non-blocking synchronous send
//                 iRCCE_push_srecv_request() (called by iRCCE_push_recv_request)
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
// FUNCTION: iRCCE_srecv_general
//--------------------------------------------------------------------------------------
// pipelined receive function
//--------------------------------------------------------------------------------------
static int iRCCE_srecv_general(
		char *privbuf,    // destination buffer in local private memory (receive buffer)
		t_vcharp combuf,  // intermediate buffer in MPB
		size_t chunk,     // size of MPB available for this message (bytes)
		RCCE_FLAG *ready, // flag indicating whether receiver is ready
		RCCE_FLAG *sent,  // flag indicating whether message has been sent by source
		ssize_t size,     // size of message (bytes)
		int source,       // UE that sent the message
		int *test         // if 1 upon entry, do nonblocking receive; if message available
		                  // set to 1, otherwise to 0
		) {

	char padline[RCCE_LINE_SIZE]; // copy buffer, used if message not multiple of line size
	size_t wsize,                 // offset within receive buffer when pulling in "chunk" bytes
	       remainder,             // bytes remaining to be received
	       nbytes;                // number of bytes to be received in single iRCCE_get call
	int first_test;               // only use first chunk to determine if message has been received yet
	char *bufptr;                 // running pointer inside privbuf for current location
	size_t subchunk1, subchunk2;  // sub-chunks for the pipelined message transfer

#ifndef _iRCCE_ANY_LENGTH_
#define FLAG_SET_VALUE RCCE_FLAG_SET
#else
	RCCE_FLAG_STATUS FLAG_SET_VALUE;

	while (1) {	 
		RCCE_flag_read(*sent, &size, RCCE_IAM);
		if(size!=0) break;
	}
	FLAG_SET_VALUE = (RCCE_FLAG_STATUS)size;
#endif

	if(iRCCE_recent_source != source) iRCCE_recent_source = source;
	if(iRCCE_recent_length != size)   iRCCE_recent_length = size;

	first_test = 1;
	  
	for (wsize=0; wsize < (size/chunk)*chunk; wsize+=chunk) {
	    	    
		if (*test && first_test) {
			first_test = 0;
			iRCCE_test_flag(*sent, RCCE_FLAG_SET, test);
			if (!(*test)) return(iRCCE_PENDING);
		}    

		if(wsize == 0) {
			// allign sub-chunks to cache line granularity:
			subchunk1 = ( (chunk / 2) / RCCE_LINE_SIZE ) * RCCE_LINE_SIZE;
			subchunk2 = chunk - subchunk1;
		}

		bufptr = privbuf + wsize;
		nbytes = subchunk1;

		RCCE_wait_until(*sent, FLAG_SET_VALUE);
		RCCE_flag_write(sent, RCCE_FLAG_UNSET, RCCE_IAM);
		RCCE_flag_write(ready, RCCE_FLAG_SET, source);
		iRCCE_get((t_vcharp)bufptr, combuf, nbytes, source);
	    
		bufptr = privbuf + wsize + subchunk1;
		nbytes = subchunk2;
	    
		RCCE_wait_until(*sent, FLAG_SET_VALUE);
		RCCE_flag_write(sent, RCCE_FLAG_UNSET, RCCE_IAM);
		RCCE_flag_write(ready, RCCE_FLAG_SET, source);
		iRCCE_get((t_vcharp)bufptr, combuf + subchunk1, nbytes, source); 	 
	}

	remainder = size%chunk; 
	// if nothing is left over, we are done 
	if (!remainder) return(iRCCE_SUCCESS);

	// receive remainder of data--whole cache lines               
	bufptr = privbuf + (size/chunk)*chunk;
	nbytes = remainder - remainder % RCCE_LINE_SIZE;
	if (nbytes) {
		// if function is called in test mode, check if first chunk has been sent already. 
		// If so, proceed as usual. If not, exit immediately 
		if (*test && first_test) {
			first_test = 0;
			iRCCE_test_flag(*sent, RCCE_FLAG_SET, test);
			if (!(*test)) return(iRCCE_PENDING);
		}

		RCCE_wait_until(*sent, FLAG_SET_VALUE);
		RCCE_flag_write(sent, RCCE_FLAG_UNSET, RCCE_IAM);
		// copy data from local MPB space to private memory 
		iRCCE_get((t_vcharp)bufptr, combuf, nbytes, source);

		// tell the source I have moved data out of its comm buffer
		RCCE_flag_write(ready, RCCE_FLAG_SET, source);
	}

	remainder = remainder % RCCE_LINE_SIZE;
	if (!remainder) return(iRCCE_SUCCESS);

	// remainder is less than cache line. This must be copied into appropriately sized 
	// intermediate space before exact number of bytes get copied to the final destination 
	bufptr = privbuf + (size/chunk)*chunk + nbytes;
	nbytes = RCCE_LINE_SIZE;

	// if function is called in test mode, check if first chunk has been sent already. 
	// If so, proceed as usual. If not, exit immediately 
	if (*test && first_test) {
		first_test = 0;
		iRCCE_test_flag(*sent, RCCE_FLAG_SET, test);
		if (!(*test)) return(iRCCE_PENDING);
	}

	RCCE_wait_until(*sent, FLAG_SET_VALUE);
	RCCE_flag_write(sent, RCCE_FLAG_UNSET, RCCE_IAM);

	// copy data from local MPB space to private memory   
	iRCCE_get((t_vcharp)padline, combuf, nbytes, source);
	memcpy_scc(bufptr, padline, remainder);    

	// tell the source I have moved data out of its comm buffer
	RCCE_flag_write(ready, RCCE_FLAG_SET, source);

	return(iRCCE_SUCCESS);
}


//--------------------------------------------------------------------------------------
// FUNCTION: iRCCE_srecv
//--------------------------------------------------------------------------------------
// pipelined recv function (blocking!)
//--------------------------------------------------------------------------------------
int iRCCE_srecv(char *privbuf, ssize_t size, int source) {

	int ignore = 0;

	if(size < 0) {
#ifdef _iRCCE_ANY_LENGTH_
		if (size != iRCCE_ANY_LENGTH)
#endif 
		{
			return(iRCCE_SUCCESS);
		}
	}

	if(size == 0) {
		// just synchronize:
		size = 1;
		privbuf = (char*)&size;
	}

	// determine source of request if given source = iRCCE_ANY_SOURCE
	if (source == iRCCE_ANY_SOURCE) {
	
	  	// wait for completion of _all_ pending non-blocking requests:
		iRCCE_irecv_wait(NULL);

		int i, res;				
		for( i=0;;i=(i+1)%RCCE_NP ){
			iRCCE_test_flag(RCCE_sent_flag[i], RCCE_FLAG_SET, &res);
			if ( (i != RCCE_IAM) && (res) ) {
				source = i;
				break;
			}
		}
	}

	// wait for completion of pending (ans source-related) non-blocking requests:
	while(iRCCE_irecv_queue[source] != NULL) {
		iRCCE_irecv_push();
		iRCCE_isend_push();
	}

#if !defined(SINGLEBITFLAGS) && !defined(RCCE_VERSION)
	if(size <= iRCCE_MAX_TAGGED_LEN) {
#ifndef _iRCCE_ANY_LENGTH_
#define FLAG_SET_VALUE RCCE_FLAG_SET
#else
		RCCE_FLAG_STATUS FLAG_SET_VALUE;

		if(size == iRCCE_ANY_LENGTH) {
			while (1) {	 
				RCCE_flag_read(RCCE_sent_flag[source], &size, RCCE_IAM);
				if(size!=0) break;
			}
		}
		FLAG_SET_VALUE = (RCCE_FLAG_STATUS)size;
#endif
		if(size <= iRCCE_MAX_TAGGED_LEN) {
			// just wait and then read the tagged flag with payload:
			iRCCE_wait_tagged(RCCE_sent_flag[source], FLAG_SET_VALUE, privbuf, size);

			RCCE_flag_write(&RCCE_sent_flag[source], RCCE_FLAG_UNSET, RCCE_IAM);
			RCCE_flag_write(&RCCE_ready_flag[RCCE_IAM], RCCE_FLAG_SET, source);  

			return(RCCE_SUCCESS);
		}
	}
#endif

	if (source<0 || source >= RCCE_NP) 
		return(RCCE_error_return(RCCE_debug_comm,RCCE_ERROR_ID));
	else {
		return(iRCCE_srecv_general(privbuf, RCCE_buff_ptr, RCCE_chunk, 
					  &RCCE_ready_flag[RCCE_IAM], &RCCE_sent_flag[source], 
					   size, source, &ignore));
	}
}

//--------------------------------------------------------------------------------------
// FUNCTION: iRCCE_probe
//--------------------------------------------------------------------------------------
// probe for incomming messages (blocking / does not receive)
//--------------------------------------------------------------------------------------
int iRCCE_probe(int source, int* test_rank)
{
	// determine source of request if given source = iRCCE_ANY_SOURCE
	if (source == iRCCE_ANY_SOURCE) {
    
		// wait for completion of _all_ pending non-blocking requests:
		iRCCE_irecv_wait(NULL);
    
		int i, res;				
		for( i=0;;i=(i+1)%RCCE_NP ){
			iRCCE_test_flag(RCCE_sent_flag[i], RCCE_FLAG_SET, &res);
			if ( (i != RCCE_IAM) && (res) ) {
				source = i;
				break;
			}
		}
	}
	else {
		int res;
		do {
			iRCCE_test_flag(RCCE_sent_flag[source], RCCE_FLAG_SET, &res);
		}
		while(!res);
	}
	
	if (test_rank != NULL) {
		(*test_rank) = source;
	}	    

#ifdef _iRCCE_ANY_LENGTH_
	{
	  ssize_t size;
	  RCCE_flag_read(RCCE_sent_flag[source], &size, RCCE_IAM);
	  if(iRCCE_recent_length != size) iRCCE_recent_length = size;
	}
#endif
	if(iRCCE_recent_source != source) iRCCE_recent_source = source;

	return  iRCCE_SUCCESS;
}

//--------------------------------------------------------------------------------------
// FUNCTION: iRCCE_recv
//--------------------------------------------------------------------------------------
// pipelined recv function (non-blocking / analogous to RCCE_recv_test fuction)
//--------------------------------------------------------------------------------------
int iRCCE_srecv_test(char *privbuf, ssize_t size, int source, int *test) {

	if(test == NULL) return iRCCE_recv(privbuf, size, source);

	if(size <= 0) {
#ifdef _iRCCE_ANY_LENGTH_
	  if(size != iRCCE_ANY_LENGTH)
#endif
		{
	    		(*test) = 1;		
			return(iRCCE_SUCCESS);
		}
	}

	// determine source of request if given source = iRCCE_ANY_SOURCE
	if (source == iRCCE_ANY_SOURCE) {

	  	// check whether there are still pending non-blocking receive requests:
		if(iRCCE_irecv_push() != iRCCE_SUCCESS) {
    			(*test) = 0;		
			return(iRCCE_PENDING);
		}

		int i, res;				
		for( i=0; i<RCCE_NP; i++){
			iRCCE_test_flag(RCCE_sent_flag[i], RCCE_FLAG_SET, &res);
			if ( (i != RCCE_IAM) && (res) ) {
				source = i;
				break;
			}
		}
	}
	if (source == iRCCE_ANY_SOURCE) {
		// currently, there is no message available (from any source):
		(*test) = 0;
		return (iRCCE_PENDING);
	}


	if(iRCCE_irecv_queue[source] != NULL) {

		// push pending non-blocking requests
		iRCCE_irecv_push();
		iRCCE_isend_push();		

		if(iRCCE_irecv_queue[source] != NULL) {
			(*test) = 0;
			return (iRCCE_PENDING);
		}
	}

	if (source<0 || source >= RCCE_NP) 
		return(RCCE_error_return(RCCE_debug_comm,RCCE_ERROR_ID));
	else {
		(*test) = 1;
		return(iRCCE_srecv_general(privbuf, RCCE_buff_ptr, RCCE_chunk, 
					  &RCCE_ready_flag[RCCE_IAM], &RCCE_sent_flag[source], 
					   size, source, test));
	}
}

//--------------------------------------------------------------------------------------
// FUNCTION: iRCCE_push_srecv_request
//--------------------------------------------------------------------------------------
// pipelined push for recv function (non-blocking and stricly synchronous!)
//--------------------------------------------------------------------------------------
int iRCCE_push_srecv_request(iRCCE_RECV_REQUEST *request) {

	char padline[RCCE_LINE_SIZE]; // copy buffer, used if message not multiple of line size
	int  test;                    // flag for calling iRCCE_test_flag()

	if(request->finished) return(iRCCE_SUCCESS);

	if(request->label == 1) goto label1;
	if(request->label == 2) goto label2;
	if(request->label == 3) goto label3;
	if(request->label == 4) goto label4;

#ifdef _iRCCE_ANY_LENGTH_
	RCCE_flag_read(*(request->sent), &(request->flag_set_value), RCCE_IAM);
	if(request->flag_set_value == 0) {
		return(iRCCE_PENDING);
	}
	request->size = (size_t)request->flag_set_value;
#endif

	// receive data in units of available chunk size of MPB 
	for (; request->wsize < (request->size / request->chunk) * request->chunk; request->wsize += request->chunk) {

		request->bufptr = request->privbuf + request->wsize;
		request->nbytes = request->subchunk1;
label1:
		iRCCE_test_flag(*(request->sent), request->flag_set_value, &test);
		if(!test) {
			request->label = 1;
			return(iRCCE_PENDING);
		}
		request->started = 1;

		RCCE_flag_write(request->sent, RCCE_FLAG_UNSET, RCCE_IAM);
		RCCE_flag_write(request->ready, RCCE_FLAG_SET, request->source);
		iRCCE_get((t_vcharp)request->bufptr, request->combuf, request->nbytes, request->source);

		request->bufptr = request->privbuf + request->wsize + request->subchunk1;
		request->nbytes = request->subchunk2;

label2:
		iRCCE_test_flag(*(request->sent), request->flag_set_value, &test);
		if(!test) {
			request->label = 2;
			return(iRCCE_PENDING);
		}

		RCCE_flag_write(request->sent, RCCE_FLAG_UNSET, RCCE_IAM);
		RCCE_flag_write(request->ready, RCCE_FLAG_SET, request->source);
		iRCCE_get((t_vcharp)request->bufptr, request->combuf + request->subchunk1, request->nbytes, request->source);
	}

	request->remainder = request->size % request->chunk; 
	// if nothing is left over, we are done 
	if (!request->remainder) {
	  	if(iRCCE_recent_source != request->source) iRCCE_recent_source = request->source;
	  	if(iRCCE_recent_length != request->size) iRCCE_recent_length = request->size;
	  	request->finished = 1;
		return(iRCCE_SUCCESS);
	}

	// receive remainder of data--whole cache lines               
	request->bufptr = request->privbuf + (request->size / request->chunk) * request->chunk;
	request->nbytes = request->remainder - request->remainder % RCCE_LINE_SIZE;
	if (request->nbytes) {
label3:
		iRCCE_test_flag(*(request->sent), request->flag_set_value, &test);
		if(!test) {
			request->label = 3;
			return(iRCCE_PENDING);
		}
		request->started = 1;

		RCCE_flag_write(request->sent, RCCE_FLAG_UNSET, RCCE_IAM);
		// copy data from source's MPB space to private memory 
		iRCCE_get((t_vcharp)request->bufptr, request->combuf, request->nbytes, request->source);

		// tell the source I have moved data out of its comm buffer
		RCCE_flag_write(request->ready, RCCE_FLAG_SET, request->source);
	}

	request->remainder = request->size % request->chunk; 
	request->remainder = request->remainder % RCCE_LINE_SIZE;
	if (!request->remainder) {
		if(iRCCE_recent_source != request->source) iRCCE_recent_source = request->source;
		if(iRCCE_recent_length != request->size)   iRCCE_recent_length = request->size;
		request->finished = 1;
		return(iRCCE_SUCCESS);
	}

	// remainder is less than cache line. This must be copied into appropriately sized 
	// intermediate space before exact number of bytes get copied to the final destination 
	request->bufptr = request->privbuf + (request->size / request->chunk) * request->chunk + request->nbytes;
	request->nbytes = RCCE_LINE_SIZE;
label4:
	iRCCE_test_flag(*(request->sent), request->flag_set_value, &test);
	if(!test) {
		request->label = 4;
		return(iRCCE_PENDING);
	}
	request->started = 1;

	RCCE_flag_write(request->sent, RCCE_FLAG_UNSET, RCCE_IAM);
	// copy data from source's MPB space to private memory   
	iRCCE_get((t_vcharp)padline, request->combuf, request->nbytes, request->source);
	memcpy_scc(request->bufptr,padline,request->remainder);

	// tell the source I have moved data out of its comm buffer
	RCCE_flag_write(request->ready, RCCE_FLAG_SET, request->source);

	if(iRCCE_recent_source != request->source) iRCCE_recent_source = request->source;
	if(iRCCE_recent_length != request->size)   iRCCE_recent_length = request->size;
	request->finished = 1;
	return(iRCCE_SUCCESS);
}

