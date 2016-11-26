/****************************************************************************************
 * Functions for a convenient handling of multiple outstanding non-blocking requests
 ****************************************************************************************
 *
 * Authors: Jacek Galowicz, Carsten Clauss
 *          Chair for Operating Systems, RWTH Aachen University
 * Date:    2010-12-09
 *
 ****************************************************************************************
 * 
 * Copyright 2010 Jacek Galowicz, Chair for Operating Systems,
 *                                RWTH Aachen University
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *     http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 *
 */

#include "iRCCE_lib.h"

void iRCCE_init_wait_list(iRCCE_WAIT_LIST *list)
{
	list->first = NULL;
	list->last = NULL;
}

static void iRCCE_add_wait_list_generic(iRCCE_WAIT_LIST *list, iRCCE_WAIT_LISTELEM * elem)
{
	if (list->first == NULL) {
		list->first = elem;
		list->last = elem;
		return;
	}

	list->last->next = elem;
	list->last = elem;
}

//--------------------------------------------------------------------------------------
// FUNCTION: iRCCE_add_recv_to_wait_list
//--------------------------------------------------------------------------------------
// Function for adding Send requests to the waitall-queue
//--------------------------------------------------------------------------------------
void iRCCE_add_send_to_wait_list(iRCCE_WAIT_LIST *list, iRCCE_SEND_REQUEST * req)
{
	iRCCE_WAIT_LISTELEM *elem;
	elem = (iRCCE_WAIT_LISTELEM*)malloc(sizeof(iRCCE_WAIT_LISTELEM));

	elem->type = iRCCE_WAIT_LIST_SEND_TYPE;
	elem->next = NULL;
	elem->req = (void*)req;
	iRCCE_add_wait_list_generic(list, elem);

	return;
}

//--------------------------------------------------------------------------------------
// FUNCTION: iRCCE_add_send_to_wait_list
//--------------------------------------------------------------------------------------
// Function for adding Recv requests to the waitall-queue
//--------------------------------------------------------------------------------------
void iRCCE_add_recv_to_wait_list(iRCCE_WAIT_LIST *list, iRCCE_RECV_REQUEST * req)
{
	iRCCE_WAIT_LISTELEM *elem;
	elem = (iRCCE_WAIT_LISTELEM*)malloc(sizeof(iRCCE_WAIT_LISTELEM));

	elem->type = iRCCE_WAIT_LIST_RECV_TYPE;
	elem->next = NULL;
	elem->req = (void*)req;
	iRCCE_add_wait_list_generic(list, elem);

	return;
}

//--------------------------------------------------------------------------------------
// FUNCTION: iRCCE_add_to_wait_list
//--------------------------------------------------------------------------------------
// Function for adding Send and/or Recv requests to the waitall-queue
//--------------------------------------------------------------------------------------
void iRCCE_add_to_wait_list(iRCCE_WAIT_LIST *list, iRCCE_SEND_REQUEST * send_req, iRCCE_RECV_REQUEST * recv_req)
{
	if (send_req != NULL) iRCCE_add_send_to_wait_list(list, send_req);
	if (recv_req != NULL) iRCCE_add_recv_to_wait_list(list, recv_req);

	return;
}

//--------------------------------------------------------------------------------------
// FUNCTION: iRCCE_wait_all
//--------------------------------------------------------------------------------------
// Blocking wait for completion of all enqueued send and recv calls
//--------------------------------------------------------------------------------------
int iRCCE_wait_all(iRCCE_WAIT_LIST *list)
{
  while(iRCCE_test_all(list, NULL) != iRCCE_SUCCESS) ;

  return iRCCE_SUCCESS;
}

//--------------------------------------------------------------------------------------
// FUNCTION: iRCCE_test_all
//--------------------------------------------------------------------------------------
// Nonblocking test for completion of all enqueued send and recv calls
// Just provide NULL instead of testvar if you don't need it
//--------------------------------------------------------------------------------------
int iRCCE_test_all(iRCCE_WAIT_LIST *list, int *test)
{
	int retval = iRCCE_SUCCESS;
	int req_state;
	iRCCE_WAIT_LISTELEM *pElem;
	iRCCE_WAIT_LISTELEM *pLastElem;
	iRCCE_WAIT_LISTELEM *pTemp;
	pLastElem = NULL;
	pElem = list->first;

	while (pElem != NULL) {
		if (pElem->type == iRCCE_WAIT_LIST_SEND_TYPE)
			req_state = iRCCE_isend_test((iRCCE_SEND_REQUEST*)pElem->req, NULL);
		else
			req_state = iRCCE_irecv_test((iRCCE_RECV_REQUEST*)pElem->req, NULL);

		if (req_state == iRCCE_SUCCESS) {
			// Remove this element from the list
			if (pElem == list->first) {
				list->first = pElem->next;
			}
			else if (pElem == list->last) {
				list->last = pLastElem;
				pLastElem->next = NULL;
			}
			else {
				pLastElem->next = pElem->next;
			}

			pTemp = pElem->next;
			free(pElem);
			pElem = pTemp;
		} 
		else {
			retval = iRCCE_PENDING;

			pLastElem = pElem;
			pElem = pElem->next;
		}
	}

	if (test) {
		if (retval ==  iRCCE_SUCCESS) {
			(*test) = 1;
		}
		else {
			(*test) = 0;
		}
	}

	return retval;
}

//--------------------------------------------------------------------------------------
// FUNCTION: iRCCE_wait_any
//--------------------------------------------------------------------------------------
// Blocking wait for completion of any enqueued send and recv request
//--------------------------------------------------------------------------------------
int iRCCE_wait_any(iRCCE_WAIT_LIST *list, iRCCE_SEND_REQUEST ** send_request, iRCCE_RECV_REQUEST ** recv_request)
{
  while(iRCCE_test_any(list, send_request, recv_request) != iRCCE_SUCCESS) ;

  return iRCCE_SUCCESS;
}

//--------------------------------------------------------------------------------------
// FUNCTION: iRCCE_test_any
//--------------------------------------------------------------------------------------
// Nonblocking test for completion of any enqueued send or recv request
//--------------------------------------------------------------------------------------
int iRCCE_test_any(iRCCE_WAIT_LIST *list, iRCCE_SEND_REQUEST ** send_request, iRCCE_RECV_REQUEST ** recv_request)
{
	int req_state;
	
	iRCCE_WAIT_LISTELEM *pElem;
	iRCCE_WAIT_LISTELEM *pLastElem;
	iRCCE_WAIT_LISTELEM *pTemp;
	pLastElem = NULL;
	pElem = list->first;

	while (pElem != NULL) {
		if (pElem->type == iRCCE_WAIT_LIST_SEND_TYPE)
			req_state = iRCCE_isend_test((iRCCE_SEND_REQUEST*)pElem->req, NULL);
		else
			req_state = iRCCE_irecv_test((iRCCE_RECV_REQUEST*)pElem->req, NULL);

		if (req_state == iRCCE_SUCCESS) {
			// Remove this element from the list
			if (pElem == list->first) {
				list->first = pElem->next;
			}
			else if (pElem == list->last) {
				list->last = pLastElem;
				pLastElem->next = NULL;
			}
			else {
				pLastElem->next = pElem->next;
			}

			if (pElem->type == iRCCE_WAIT_LIST_SEND_TYPE) {
				if(send_request) {
					(*send_request) = (iRCCE_SEND_REQUEST*)pElem->req;
				}
				if(recv_request) {
					(*recv_request) = NULL;
				}
			}
			else {
				if(send_request) {
					(*send_request) = NULL;
				}
				if(recv_request) {
					(*recv_request) = (iRCCE_RECV_REQUEST*)pElem->req;
				}
			}

			pTemp = pElem->next;
			free(pElem);
			pElem = pTemp;
			
			return iRCCE_SUCCESS;
		} 
		else {
			pLastElem = pElem;
			pElem = pElem->next;
		}
	}

	if(send_request) {
		(*send_request) = NULL;
	}
	if(recv_request) {
		(*recv_request) = NULL;
	}

	return iRCCE_PENDING;
}


//--------------------------------------------------------------------------------------
// FUNCTIONS: iRCCE_get_dest, iRCCE_get_source, iRCCE_get_length, iRCCE_get_status
//--------------------------------------------------------------------------------------
// Functions to determine the respective sender/receiver after test_any() / wait_any()
// (Can also be used after receiving a message via wildcard mechanism!)
//--------------------------------------------------------------------------------------
int iRCCE_get_dest(iRCCE_SEND_REQUEST *request)
{
	if(request != NULL) return request->dest;

	return iRCCE_ERROR;
}
//--------------------------------------------------------------------------------------
int iRCCE_get_source(iRCCE_RECV_REQUEST *request)
{
	if(request != NULL) return request->source;
  
	return iRCCE_recent_source;
}
//--------------------------------------------------------------------------------------
int iRCCE_get_size(iRCCE_SEND_REQUEST * send_req, iRCCE_RECV_REQUEST * recv_req)
{
	if(send_req != NULL) return send_req->size;
	if(recv_req != NULL) return recv_req->size;   
  
	return iRCCE_recent_length;
}
//--------------------------------------------------------------------------------------
int iRCCE_get_length(void)
{
	return iRCCE_recent_length;
}
//--------------------------------------------------------------------------------------
int iRCCE_get_status(iRCCE_SEND_REQUEST * send_req, iRCCE_RECV_REQUEST * recv_req)
{
	if(send_req != NULL) {
  
		if(send_req->finished) {

		  	return(iRCCE_SUCCESS);
		}

		if(iRCCE_isend_queue != send_req) {

		  	return(iRCCE_RESERVED);
		}
		else
		{
	  		return(iRCCE_PENDING);
		}		
	}

	if(recv_req != NULL) {
	  
		if(recv_req->finished) {

		    	return(iRCCE_SUCCESS);
		}

		if(iRCCE_irecv_queue[recv_req->source] != recv_req) {

	  		return(iRCCE_RESERVED);
		}
		else
		{
	  		return(iRCCE_PENDING);
		}		    
	}

	return iRCCE_ERROR;
}
