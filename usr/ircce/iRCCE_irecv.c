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
//                 - iRCCE_isend(), ..._test(), ..._wait(), ..._push()
//                 - iRCCE_irecv(), ..._test(), ..._wait(), ..._push()
//                 by Carsten Clauss, Chair for Operating Systems,
//                                    RWTH Aachen University
//
//    [2010-11-12] extracted non-blocking code into separate library
//                 by Carsten Scholtes
//
//    [2010-12-09] added cancel functions for non-blocking send/recv requests
//                 by Carsten Clauss
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
//    [2011-08-02] added iRCCE_iprobe() function for probing for incomming messages
//
//    [2011-11-03] added non-blocking by synchronous send/recv functions:
//                 iRCCE_issend() / iRCCE_isrecv()
//

#include "iRCCE_lib.h"

#ifdef __hermit__
#include "rte_memcpy.h"
#define memcpy_scc rte_memcpy
#elif defined COPPERRIDGE || defined SCC
#include "scc_memcpy.h"
#else
#define memcpy_scc memcpy
#endif

#ifdef SINGLEBITFLAGS
#warning iRCCE_ANY_LENGTH: for using this wildcard, SINGLEBITFLAGS must be disabled! (make SINGLEBITFLAGS=0)
#endif

#ifdef RCCE_VERSION
#warning iRCCE_ANY_LENGTH: for using this wildcard, iRCCE must be built against RCCE release V1.0.13!
#endif

static int iRCCE_push_recv_request(iRCCE_RECV_REQUEST *request) {

	char padline[RCCE_LINE_SIZE]; // copy buffer, used if message not multiple of line size
	int  test;                    // flag for calling iRCCE_test_flag()

	if(request->finished) return(iRCCE_SUCCESS);

	if(request->sync) return iRCCE_push_srecv_request(request);

	if(request->label == 1) goto label1;
	if(request->label == 2) goto label2;
	if(request->label == 3) goto label3;

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
		request->nbytes = request->chunk;
label1:
		iRCCE_test_flag(*(request->sent), request->flag_set_value, &test);
		if(!test) {
			request->label = 1;
			return(iRCCE_PENDING);
		}
		request->started = 1;

		RCCE_flag_write(request->sent, RCCE_FLAG_UNSET, RCCE_IAM);
		// copy data from source's MPB space to private memory 
		iRCCE_get((t_vcharp)request->bufptr, request->combuf, request->nbytes, request->source);

		// tell the source I have moved data out of its comm buffer
		RCCE_flag_write(request->ready, request->flag_set_value, request->source);
	}

	request->remainder = request->size % request->chunk; 
	// if nothing is left over, we are done 
	if (!request->remainder) {
		if(iRCCE_recent_source != request->source) iRCCE_recent_source = request->source;
		if(iRCCE_recent_length != request->size)   iRCCE_recent_length = request->size;
		request->finished = 1;
		return(iRCCE_SUCCESS);
	}

	// receive remainder of data--whole cache lines               
	request->bufptr = request->privbuf + (request->size / request->chunk) * request->chunk;
	request->nbytes = request->remainder - request->remainder % RCCE_LINE_SIZE;
	if (request->nbytes) {
label2:
		iRCCE_test_flag(*(request->sent), request->flag_set_value, &test);
		if(!test) {
			request->label = 2;
			return(iRCCE_PENDING);
		}
		request->started = 1;

		RCCE_flag_write(request->sent, RCCE_FLAG_UNSET, RCCE_IAM);
		// copy data from source's MPB space to private memory 
		iRCCE_get((t_vcharp)request->bufptr, request->combuf, request->nbytes, request->source);

		// tell the source I have moved data out of its comm buffer
		RCCE_flag_write(request->ready, request->flag_set_value, request->source);
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
label3:
	iRCCE_test_flag(*(request->sent), request->flag_set_value, &test);
	if(!test) {
		request->label = 3;
		return(iRCCE_PENDING);
	}
	request->started = 1;

	RCCE_flag_write(request->sent, RCCE_FLAG_UNSET, RCCE_IAM);
	// copy data from source's MPB space to private memory   
	iRCCE_get((t_vcharp)padline, request->combuf, request->nbytes, request->source);
	memcpy_scc(request->bufptr,padline,request->remainder);

	// tell the source I have moved data out of its comm buffer
	RCCE_flag_write(request->ready, request->flag_set_value, request->source);

	if(iRCCE_recent_source != request->source) iRCCE_recent_source = request->source;
	if(iRCCE_recent_length != request->size)   iRCCE_recent_length = request->size;
	request->finished = 1;
	return(iRCCE_SUCCESS);
}

static void iRCCE_init_recv_request(
		char *privbuf,    // source buffer in local private memory (send buffer)
		t_vcharp combuf,  // intermediate buffer in MPB
		size_t chunk,     // size of MPB available for this message (bytes)
		RCCE_FLAG *ready, // flag indicating whether receiver is ready
		RCCE_FLAG *sent,  // flag indicating whether message has been sent by source
		size_t size,      // size of message (bytes)
		int source,       // UE that will send the message
		int sync,         // flag indicating whether recv is synchronous or not
		iRCCE_RECV_REQUEST *request
		) {

	request->privbuf   = privbuf;
	request->combuf    = combuf;
	request->chunk     = chunk;
	request->ready     = ready;
	request->sent      = sent;
	request->size      = size;
	request->source    = source;

	request->sync      = sync;	
	request->subchunk1 = chunk / 2;
	request->subchunk1 = ( (chunk / 2) / RCCE_LINE_SIZE ) * RCCE_LINE_SIZE;
	request->subchunk2 = chunk - request->subchunk1;

	request->wsize     = 0;
	request->remainder = 0;
	request->nbytes    = 0;
	request->bufptr    = NULL;

	request->label     = 0;
	request->finished  = 0;
	request->started   = 0;

	request->next      = NULL;

#ifndef _iRCCE_ANY_LENGTH_
	request->flag_set_value = RCCE_FLAG_SET;
#else
	request->flag_set_value = (RCCE_FLAG_STATUS)size;
#endif

	return;
}

static int iRCCE_irecv_search_source() {
	int i, j; 
	int res = iRCCE_ANY_SOURCE;

	for( i=0; i<RCCE_NP*3; ++i ){
		j =i%RCCE_NP;
		if ( j == RCCE_IAM )
			continue;

		// only take source if recv-queue is empty
		if(!iRCCE_irecv_queue[j]) {
			int test;
			iRCCE_test_flag(RCCE_sent_flag[j], 0, &test);
			if(!test) {
				res = j;
				break;
			}
		}
	}

	return res;
}


//--------------------------------------------------------------------------------------
// FUNCTION: iRCCE_irecv
//--------------------------------------------------------------------------------------
// non-blocking recv function; returns an handle of type iRCCE_RECV_REQUEST
//--------------------------------------------------------------------------------------
static iRCCE_RECV_REQUEST blocking_irecv_request;
#ifdef _OPENMP
#pragma omp threadprivate (blocking_irecv_request)
#endif
inline static int iRCCE_irecv_generic(char *privbuf, ssize_t size, int source, iRCCE_RECV_REQUEST *request, int sync) {

	if(request == NULL){
		request = &blocking_irecv_request;
	
		// find source (blocking)	
		if( source == iRCCE_ANY_SOURCE ){
			int i;
			for( i=0;;i=(i+1)%RCCE_NP ){

				if( (!iRCCE_irecv_queue[i]) && (i != RCCE_IAM) ) {
					int test;
					iRCCE_test_flag(RCCE_sent_flag[i], 0, &test);
					if(!test) {
						source = i;
						break;
					}
				} 			    
			}
		}
	}

	if(size == 0) {
		if(sync) {
			// just synchronize:
			size = 1;
			privbuf = (char*)&size;
		} else
		  	size = -1;
	}

	if(size <= 0) {
#ifdef _iRCCE_ANY_LENGTH_
		if(size != iRCCE_ANY_LENGTH)
#endif
		{
			iRCCE_init_recv_request(privbuf, RCCE_buff_ptr, RCCE_chunk, 
						&RCCE_ready_flag[RCCE_IAM], &RCCE_sent_flag[source], 
						size, source, sync, request);
			request->finished = 1;
			return(iRCCE_SUCCESS);
		}
	}

	if( source == iRCCE_ANY_SOURCE ) {
		source = iRCCE_irecv_search_source(); // first try to find a source
	
		if( source == iRCCE_ANY_SOURCE ){ // queue request if no source available

			iRCCE_init_recv_request(privbuf, RCCE_buff_ptr, RCCE_chunk,
						&RCCE_ready_flag[RCCE_IAM], NULL,
						size, iRCCE_ANY_SOURCE, sync, request);
	
			// put anysource-request in irecv_any_source_queue	
			if( iRCCE_irecv_any_source_queue == NULL ){
				iRCCE_irecv_any_source_queue = request;
			}
			else {
				if( iRCCE_irecv_any_source_queue->next  == NULL ) {
					iRCCE_irecv_any_source_queue->next = request;
				}
				else {
					iRCCE_RECV_REQUEST* run = iRCCE_irecv_any_source_queue;
					while( run->next != NULL ) run = run->next;
					run->next = request;
				}
			}
			return iRCCE_RESERVED;
		}
	}

	if (source<0 || source >= RCCE_NP) 
		return(RCCE_error_return(RCCE_debug_comm,RCCE_ERROR_ID));
	else {
		iRCCE_init_recv_request(privbuf, RCCE_buff_ptr, RCCE_chunk, 
					&RCCE_ready_flag[RCCE_IAM], &RCCE_sent_flag[source], 
					size, source, sync, request);

		if(iRCCE_irecv_queue[source] == NULL) {

			if(iRCCE_push_recv_request(request) == iRCCE_SUCCESS) {
				return(iRCCE_SUCCESS);
			}
			else {       
				iRCCE_irecv_queue[source] = request;

				if(request == &blocking_irecv_request) {
					iRCCE_irecv_wait(request);
					return(iRCCE_SUCCESS);
				}

				return(iRCCE_PENDING);
			}
		}
		else {
			if(iRCCE_irecv_queue[source]->next == NULL) {
				iRCCE_irecv_queue[source]->next = request;
			}
			else {
				iRCCE_RECV_REQUEST *run = iRCCE_irecv_queue[source];
				while(run->next != NULL) run = run->next;      
				run->next = request;   
			}

			if(request == &blocking_irecv_request) {
				iRCCE_irecv_wait(request);
				return(iRCCE_SUCCESS);
			}

			return(iRCCE_RESERVED);
		}
	}
}

int iRCCE_irecv(char *privbuf, ssize_t size, int dest, iRCCE_RECV_REQUEST *request) {

	return iRCCE_irecv_generic(privbuf, size, dest, request, 0);
}

int iRCCE_isrecv(char *privbuf, ssize_t size, int dest, iRCCE_RECV_REQUEST *request) {

	return iRCCE_irecv_generic(privbuf, size, dest, request, 1);
}


//--------------------------------------------------------------------------------------
// FUNCTION: iRCCE_probe
//--------------------------------------------------------------------------------------
// probe for incomming messages (non-blocking / does not receive)
//--------------------------------------------------------------------------------------
int iRCCE_iprobe(int source, int* test_rank, int* test_flag)
{
	// determine source of request if given source = iRCCE_ANY_SOURCE
	if( source == iRCCE_ANY_SOURCE ) {

    		source = iRCCE_irecv_search_source(); // first try to find a source
	}
	else {
		int res;
		iRCCE_test_flag(RCCE_sent_flag[source], RCCE_FLAG_SET, &res);

		if(!res) source = iRCCE_ANY_SOURCE;
	}

	if(source != iRCCE_ANY_SOURCE) { // message found:

	  	if (test_rank != NULL) (*test_rank) = source;
		if (test_flag != NULL) (*test_flag) = 1;

#ifdef _iRCCE_ANY_LENGTH_
		{
			ssize_t size = iRCCE_ANY_LENGTH;
			RCCE_flag_read(RCCE_sent_flag[source], &size, RCCE_IAM);
			if(iRCCE_recent_length != size) iRCCE_recent_length = size;
		}
#endif
		if(iRCCE_recent_source != source) iRCCE_recent_source = source;
	}
	else {
		if (test_rank != NULL) (*test_rank) = iRCCE_ANY_SOURCE;
		if (test_flag != NULL) (*test_flag) = 0;		
	}
	
	return  iRCCE_SUCCESS;
}


//--------------------------------------------------------------------------------------
// FUNCTION: iRCCE_irecv_test
//--------------------------------------------------------------------------------------
// test function for completion of the requestes non-blocking recv operation
// Just provide NULL instead of the testvar if you don't need it
//--------------------------------------------------------------------------------------
int iRCCE_irecv_test(iRCCE_RECV_REQUEST *request, int *test) {

	int source;

	if(request == NULL) {

		if(iRCCE_irecv_push() == iRCCE_SUCCESS) {
			if (test) (*test) = 1;
			return(iRCCE_SUCCESS);
		}
		else {
			if (test) (*test) = 0;
			return(iRCCE_PENDING);
		}    
	}

	// does request still have no source? 
	if( request->source == iRCCE_ANY_SOURCE ) {
		request->source = iRCCE_irecv_search_source();

		if( request->source == iRCCE_ANY_SOURCE ) {
			if (test) (*test) = 0;
			return iRCCE_RESERVED;	
		}	
		else { // take request out of wait_any_source-list

			// find request in queue
			if( request == iRCCE_irecv_any_source_queue ) {
				iRCCE_irecv_any_source_queue = iRCCE_irecv_any_source_queue->next;
			}
			else {
				iRCCE_RECV_REQUEST* run = iRCCE_irecv_any_source_queue;
				while( run->next != request ) run = run->next;
				run->next = request->next;
			}
			
			request->next = NULL;
			request->sent = &RCCE_sent_flag[request->source]; // set senders flag
			source = request->source;	
			
			// queue request in iRCCE_irecv_queue
			if(iRCCE_irecv_queue[source] == NULL) {

				if(iRCCE_push_recv_request(request) == iRCCE_SUCCESS) {
					if (test) (*test) = 1;
					return(iRCCE_SUCCESS);
				}
				else {       
					iRCCE_irecv_queue[source] = request;

					if(request == &blocking_irecv_request) {
						iRCCE_irecv_wait(request);
						if (test) (*test) = 1;
						return(iRCCE_SUCCESS);
					}
					if (test) (*test) = 0;
					return(iRCCE_PENDING);
				}
			}
			else {
				if(iRCCE_irecv_queue[source]->next == NULL) {
					iRCCE_irecv_queue[source]->next = request;
				}
				else {
					iRCCE_RECV_REQUEST *run = iRCCE_irecv_queue[source];
					while(run->next != NULL) run = run->next;      
					run->next = request;   
				}

				if(request == &blocking_irecv_request) {
					iRCCE_irecv_wait(request);
					if (test) (*test) = 1;
					return(iRCCE_SUCCESS);
				}

				if (test) (*test) = 1;
				return(iRCCE_RESERVED);
			}


		}
	}
	else {

		source = request->source;

		if(request->finished) {
			if (test) (*test) = 1;
			return(iRCCE_SUCCESS);
		}

		if(iRCCE_irecv_queue[source] != request) {
			if (test) (*test) = 0;
			return(iRCCE_RESERVED);
		}

		iRCCE_push_recv_request(request);

		if(request->finished) {
			iRCCE_irecv_queue[source] = request->next;

			if (test) (*test) = 1;
			return(iRCCE_SUCCESS);
		}

		if (test) (*test) = 0;
		return(iRCCE_PENDING);
	}
}


//--------------------------------------------------------------------------------------
// FUNCTION: iRCCE_irecv_push
//--------------------------------------------------------------------------------------
// progress function for pending requests in the irecv queue 
//--------------------------------------------------------------------------------------
static int iRCCE_irecv_push_source(int source) {

	iRCCE_RECV_REQUEST *request = iRCCE_irecv_queue[source];

	if(request == NULL) {
		return(iRCCE_SUCCESS);
	}

	if(request->finished) {
		return(iRCCE_SUCCESS);
	}

	iRCCE_push_recv_request(request);   

	if(request->finished) {    
		iRCCE_irecv_queue[source] = request->next;
		return(iRCCE_SUCCESS);
	}

	return(iRCCE_PENDING);
}

int iRCCE_irecv_push(void) {
	iRCCE_RECV_REQUEST* help_request;
	
	// first check sourceless requests
	if( iRCCE_irecv_any_source_queue != NULL) {
		while( iRCCE_irecv_any_source_queue != NULL ) {
			iRCCE_irecv_any_source_queue->source = iRCCE_irecv_search_source();

			if( iRCCE_irecv_any_source_queue->source == iRCCE_ANY_SOURCE ) {

				break;
			}
			// source found for first request in iRCCE_irecv_any_source_queue
			else { 
				// set senders flag
				iRCCE_irecv_any_source_queue->sent = &RCCE_sent_flag[iRCCE_irecv_any_source_queue->source]; 
				
				// take request out of irecv_any_source_queue
				help_request = iRCCE_irecv_any_source_queue;
				iRCCE_irecv_any_source_queue = iRCCE_irecv_any_source_queue->next;
				help_request->next = NULL;				
				
				// put request into irecv_queue
				if(iRCCE_irecv_queue[help_request->source] == NULL) {
					iRCCE_irecv_queue[help_request->source] = help_request;
				}
				else {
					iRCCE_RECV_REQUEST *run = iRCCE_irecv_queue[help_request->source];
					while(run->next != NULL) run = run->next;      
					run->next = help_request;   
				}
			}
		}

	}

	int i, j; 
	int retval = iRCCE_SUCCESS;

	for(i=0; i<RCCE_NP; i++) {

		j = iRCCE_irecv_push_source(i);

		if(j != iRCCE_SUCCESS) {
			retval = j;
		}
	}

	return (iRCCE_irecv_any_source_queue == NULL)? retval : iRCCE_RESERVED;
}

//--------------------------------------------------------------------------------------
// FUNCTION: iRCCE_irecv_wait
//--------------------------------------------------------------------------------------
// just wait for completion of the requested non-blocking send operation
//--------------------------------------------------------------------------------------
int iRCCE_irecv_wait(iRCCE_RECV_REQUEST *request) {

	if(request != NULL) {
		while(!request->finished) {
			iRCCE_irecv_push();
			iRCCE_isend_push();
		}
	}
	else {
		do {
			iRCCE_isend_push();
		}
		while(  iRCCE_irecv_push() != iRCCE_SUCCESS );
	}

	return(iRCCE_SUCCESS);
}


//--------------------------------------------------------------------------------------
// FUNCTION: iRCCE_irecv_cancel
//--------------------------------------------------------------------------------------
// try to cancel a pending non-blocking recv request
//--------------------------------------------------------------------------------------
int iRCCE_irecv_cancel(iRCCE_RECV_REQUEST *request, int *test) {
  
	int source;
	iRCCE_RECV_REQUEST *run;
  
	if( (request == NULL) || (request->finished) ) {
		if (test) (*test) = 0;
		return iRCCE_NOT_ENQUEUED;
	}


	// does request have any source specified?
	if( request->source == iRCCE_ANY_SOURCE ) {
		for( run = iRCCE_irecv_any_source_queue; run->next != NULL; run = run->next ) {
			if( run->next == request ) {
				run->next = run->next->next;

				if (test) (*test) = 1;
				return iRCCE_SUCCESS;
			}
		}
	
		if (test) (*test) = 0;
		return iRCCE_NOT_ENQUEUED;
	}
	


	source = request->source;
  
	if(iRCCE_irecv_queue[source] == NULL) {
		if (test) (*test) = 0;
		return iRCCE_NOT_ENQUEUED;
	}
  
	if(iRCCE_irecv_queue[source] == request) {

		// have parts of the message already been received?
		if(request->started) {
			if (test) (*test) = 0;
			return iRCCE_PENDING;
		}
		else {
			// no, thus request can be canceld just in time:
			iRCCE_irecv_queue[source] = request->next;
			if (test) (*test) = 1;
			return iRCCE_SUCCESS;
		}
	}
 
	for(run = iRCCE_irecv_queue[source]; run->next != NULL; run = run->next) {
    
		// request found --> remove it from recv queue:
		if(run->next == request) {
      
			run->next = run->next->next;
      
			if (test) (*test) = 1;
			return iRCCE_SUCCESS;
		}
	}
  
	if (test) (*test) = 0;
	return iRCCE_NOT_ENQUEUED;
}


