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
//    [2010-11-26] added xxx
//                 by Carsten Clauss, Chair for Operating Systems,
//                                    RWTH Aachen University
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

static int iRCCE_msend_general(
		char *privbuf,    // source buffer in local private memory (send buffer)
		t_vcharp combuf,  // intermediate buffer in MPB
		size_t chunk,     // size of MPB available for this message (bytes)
		RCCE_FLAG *sent,  // flag indicating whether message has been sent by source
		ssize_t size      // size of message (bytes)
		) {

	char padline[RCCE_LINE_SIZE]; // copy buffer, used if message not multiple of line size
	size_t wsize,                 // offset within send buffer when putting in "chunk" bytes
               remainder,             // bytes remaining to be sent
	       nbytes;                // number of bytes to be sent in single iRCCE_put call
	char *bufptr;                 // running pointer inside privbuf for current location
	size_t subchunk1, subchunk2;  // sub-chunks for the pipelined message transfer
	int ue;

#ifndef _iRCCE_ANY_LENGTH_
#define FLAG_SET_VALUE RCCE_FLAG_SET
#else
	RCCE_FLAG_STATUS FLAG_SET_VALUE = (RCCE_FLAG_STATUS)size;
#endif
	// send data in units of available chunk size of comm buffer 
	for (wsize=0; wsize< (size/chunk)*chunk; wsize+=chunk) {

		bufptr = privbuf + wsize;
		nbytes = chunk;

		// copy private data to own comm buffer
		RCCE_put(combuf, (t_vcharp) bufptr, nbytes, RCCE_IAM);

		for(ue=0; ue<RCCE_NP; ue++)
		  if(ue!=RCCE_IAM) RCCE_flag_write(sent, FLAG_SET_VALUE, ue);

		iRCCE_barrier(NULL);
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
		for(ue=0; ue<RCCE_NP; ue++)
		  if(ue!=RCCE_IAM) RCCE_flag_write(sent, FLAG_SET_VALUE, ue);

		iRCCE_barrier(NULL);
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

	for(ue=0; ue<RCCE_NP; ue++)
	  if(ue!=RCCE_IAM) RCCE_flag_write(sent, FLAG_SET_VALUE, ue);

	iRCCE_barrier(NULL);

	return(iRCCE_SUCCESS);
}

//--------------------------------------------------------------------------------------
// FUNCTION: iRCCE_msend
//--------------------------------------------------------------------------------------
// pipelined multicast send function (blocking and synchronous!)
//--------------------------------------------------------------------------------------
int iRCCE_msend(char *privbuf, ssize_t size) {

	if(size <= 0) return(iRCCE_SUCCESS);

	while(iRCCE_isend_queue != NULL) {

		// wait for completion of pending non-blocking requests
		iRCCE_isend_push();
		iRCCE_irecv_push();
	}

	return(iRCCE_msend_general(privbuf, RCCE_buff_ptr, RCCE_chunk, 
				   &RCCE_sent_flag[RCCE_IAM], size));
}


//--------------------------------------------------------------------------------------
// FUNCTION: iRCCE_mrecv_general
//--------------------------------------------------------------------------------------
//  multicast receive function
//--------------------------------------------------------------------------------------
static int iRCCE_mrecv_general(
		char *privbuf,    // destination buffer in local private memory (receive buffer)
		t_vcharp combuf,  // intermediate buffer in MPB
		size_t chunk,     // size of MPB available for this message (bytes)
		RCCE_FLAG *sent,  // flag indicating whether message has been sent by source
		ssize_t size,     // size of message (bytes)
		int source        // UE that sent the message
		) {

	char padline[RCCE_LINE_SIZE]; // copy buffer, used if message not multiple of line size
	size_t wsize,                 // offset within receive buffer when pulling in "chunk" bytes
	       remainder,             // bytes remaining to be received
	       nbytes;                // number of bytes to be received in single iRCCE_get call
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

	// receive data in units of available chunk size of MPB 
	for (wsize=0; wsize< (size/chunk)*chunk; wsize+=chunk) {

		bufptr = privbuf + wsize;
		nbytes = chunk;
	  
		RCCE_wait_until(*sent, RCCE_FLAG_SET);
		RCCE_flag_write(sent, RCCE_FLAG_UNSET, RCCE_IAM);
	  
		// copy data from local MPB space to private memory 
		RCCE_get((t_vcharp)bufptr, combuf, nbytes, source);
		
		iRCCE_barrier(NULL);
	}

	remainder = size%chunk; 
	// if nothing is left over, we are done 
	if (!remainder) return(iRCCE_SUCCESS);

	// receive remainder of data--whole cache lines               
	bufptr = privbuf + (size/chunk)*chunk;
	nbytes = remainder - remainder % RCCE_LINE_SIZE;
	if (nbytes) {
	
		RCCE_wait_until(*sent, FLAG_SET_VALUE);
		RCCE_flag_write(sent, RCCE_FLAG_UNSET, RCCE_IAM);

		// copy data from local MPB space to private memory 
		iRCCE_get((t_vcharp)bufptr, combuf, nbytes, source);

		iRCCE_barrier(NULL);
	}

	remainder = remainder % RCCE_LINE_SIZE;
	if (!remainder) return(iRCCE_SUCCESS);

	// remainder is less than cache line. This must be copied into appropriately sized 
	// intermediate space before exact number of bytes get copied to the final destination 
	bufptr = privbuf + (size/chunk)*chunk + nbytes;
	nbytes = RCCE_LINE_SIZE;

	RCCE_wait_until(*sent, FLAG_SET_VALUE);
	RCCE_flag_write(sent, RCCE_FLAG_UNSET, RCCE_IAM);

	// copy data from local MPB space to private memory   
	iRCCE_get((t_vcharp)padline, combuf, nbytes, source);
	memcpy_scc(bufptr, padline, remainder);    

	iRCCE_barrier(NULL);

	return(iRCCE_SUCCESS);
}
	  
//--------------------------------------------------------------------------------------
// FUNCTION: iRCCE_mrecv
//--------------------------------------------------------------------------------------
// multicast recv function (blocking!)
//--------------------------------------------------------------------------------------
int iRCCE_mrecv(char *privbuf, ssize_t size, int source) {

	int ignore = 0;

	if(size <= 0) {
#ifdef _iRCCE_ANY_LENGTH_
		if (size != iRCCE_ANY_LENGTH)
#endif 
		{
			return(iRCCE_SUCCESS);
		}
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

	if (source<0 || source >= RCCE_NP) 
		return(RCCE_error_return(RCCE_debug_comm,RCCE_ERROR_ID));
	else {
		return(iRCCE_mrecv_general(privbuf, RCCE_buff_ptr, RCCE_chunk, 
					   &RCCE_sent_flag[source], size, source));
	}
}


//--------------------------------------------------------------------------------------
// FUNCTION: iRCCE_mcast
//--------------------------------------------------------------------------------------
// multicast based on msend() and mrecv()
//--------------------------------------------------------------------------------------
int iRCCE_mcast(char *buf, size_t size, int root)
{
	if(RCCE_IAM != root) {
		return iRCCE_mrecv(buf, size, root);
	} else {
		return iRCCE_msend(buf, size);
	}
}

//--------------------------------------------------------------------------------------
// FUNCTION: iRCCE_bcast
//--------------------------------------------------------------------------------------
// wrapper function for using iRCCE's multicast feature
//--------------------------------------------------------------------------------------
int iRCCE_bcast(char *buf, size_t size, int root, RCCE_COMM comm)
{
	if(memcmp(&comm, &RCCE_COMM_WORLD, sizeof(RCCE_COMM)) == 0) {
		return RCCE_bcast(buf, size, root, comm);
	} else {
		return iRCCE_mcast(buf, size, root);
	}
}
