//***************************************************************************************
// Broadcast functions. 
//***************************************************************************************
// Since only collective operations require communication domains, they are the only ones 
// that use communicators. All collectives implementations are naive, linear operations. 
// There may not be any overlap between target and source.
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

#include "RCCE_lib.h"

#ifdef USE_RCCE_COMM
#ifndef GORY
#include "RCCE_comm/RCCE_bcast.c"
#endif
#else

#include <stdlib.h>
#include <string.h>

//--------------------------------------------------------------------------------------
// RCCE_bcast
//--------------------------------------------------------------------------------------
// function that sends data from UE root to all other UEs in the communicator
//--------------------------------------------------------------------------------------
int RCCE_bcast(
  char *buf,     // private memory, used for sending (root) and receiving (other UEs) 
  size_t num,    // number of bytes to be sent
  int root,      // source within "comm" of broadcast data
  RCCE_COMM comm // communication domain
  ) {

  int ue, ierr;
#ifdef GORY
  printf("Collectives only implemented for simplified API\n");
  return(1);
#else
  // check to make sure root is member of the communicator
  if (root<0 || root >= comm.size) 
  return(RCCE_error_return(RCCE_debug_comm,RCCE_ERROR_ID));

  if (RCCE_IAM == comm.member[root]) {
    for (ue=0; ue<comm.size; ue++) if (ue != root)
      if((ierr=RCCE_send(buf, num, comm.member[ue])))
         return(RCCE_error_return(RCCE_debug_comm,ierr));
  }
  else if((ierr=RCCE_recv(buf, num, comm.member[root])))
         return(RCCE_error_return(RCCE_debug_comm,ierr));

  return(RCCE_SUCCESS);
#endif
}

#endif
