//***************************************************************************************
// Reduction functions. 
//***************************************************************************************
// Since reduction is the only message passing operation that depends on the data type, 
// it is carried as a parameter. Also, since only collective operations require
// communication domains, they are the only ones that use communicators. All collectives 
// implementations are naive, linear operations. There may not be any overlap between
// target and source.
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
#define MIN(x,y) ( (x) < (y) ? (x) : (y) )
#define MAX(x,y) ( (x) > (y) ? (x) : (y) )

#include <stdlib.h>
#include <string.h>

//--------------------------------------------------------------------------------------
// FUNCTION: RCCE_reduce_general
//--------------------------------------------------------------------------------------
//  function used to implement both reduce and allreduce
//--------------------------------------------------------------------------------------
static int RCCE_reduce_general(
  char *inbuf,   // source buffer for reduction datan
  char *outbuf,  // target buffer for reduction data
  int num,       // number of data elements to be reduced
  int type,      // type of data elements
  int op,        // reduction operation
  int root,      // root of reduction tree, used for all reductions
  int all,       // if 1, use allreduce, if 0, use reduce
  RCCE_COMM comm // communication domain within which to reduce
  ) {

  int ue, i, type_size, ierr;
  int    *iin, *iout;
  long   *lin, *lout;
  float  *fin, *fout;
  double *din, *dout;
  // create aliases for source and target buffers to simplify arithmetic operations
  iin = (int *)    inbuf; iout = (int *)    outbuf;
  lin = (long *)   inbuf; lout = (long *)   outbuf;
  fin = (float *)  inbuf; fout = (float *)  outbuf;
  din = (double *) inbuf; dout = (double *) outbuf;

#ifdef GORY
  printf("Reduction only implemented for non-gory API\n");
  return(1);
#else
  switch (op) {
     case RCCE_SUM:  
     case RCCE_MAX:  
     case RCCE_MIN:  
     case RCCE_PROD: break;
     default:  return(RCCE_ERROR_ILLEGAL_OP);
  }

  switch (type) {
    case RCCE_INT:    type_size = sizeof(int);    
                      break;
    case RCCE_LONG:   type_size = sizeof(long);   
                      break;
    case RCCE_FLOAT:  type_size = sizeof(float);  
                      break;
    case RCCE_DOUBLE: type_size = sizeof(double); 
                      break;
    default: return(RCCE_ERROR_ILLEGAL_TYPE);
  }

  if (RCCE_IAM != comm.member[root]) {
    // non-root UEs send their source buffers to the root
    if ((ierr=RCCE_send(inbuf, num*type_size, comm.member[root])))
      return(ierr);
    // in case of allreduce they also receive the reduced buffer
    if (all) if ((ierr=RCCE_recv(outbuf, num*type_size, comm.member[root])))
      return(ierr);
  }
  else {
    // the root can copy directly from source to target buffer
    memcpy(outbuf, inbuf, num*type_size);
    for (ue=0; ue<comm.size; ue++) if (ue != root) {
      if ((ierr=RCCE_recv(inbuf, num*type_size, comm.member[ue])))
        return(ierr);
      
      // use combination of operation and data type to reduce number of switch statements
      switch (op+(RCCE_NUM_OPS)*(type)) {

        case RCCE_SUM_INT:     for (i=0; i<num; i++) iout[i] += iin[i];             break;
        case RCCE_MAX_INT:     for (i=0; i<num; i++) iout[i] = MAX(iout[i],iin[i]); break;
        case RCCE_MIN_INT:     for (i=0; i<num; i++) iout[i] = MIN(iout[i],iin[i]); break;
        case RCCE_PROD_INT:    for (i=0; i<num; i++) iout[i] *= iin[i];             break;

        case RCCE_SUM_LONG:    for (i=0; i<num; i++) lout[i] += lin[i];             break;
        case RCCE_MAX_LONG:    for (i=0; i<num; i++) lout[i] = MAX(lout[i],lin[i]); break;
        case RCCE_MIN_LONG:    for (i=0; i<num; i++) lout[i] = MIN(lout[i],lin[i]); break;
        case RCCE_PROD_LONG:   for (i=0; i<num; i++) lout[i] *= lin[i];             break;

        case RCCE_SUM_FLOAT:   for (i=0; i<num; i++) fout[i] += fin[i];             break;
        case RCCE_MAX_FLOAT:   for (i=0; i<num; i++) fout[i] = MAX(fout[i],fin[i]); break;
        case RCCE_MIN_FLOAT:   for (i=0; i<num; i++) fout[i] = MIN(fout[i],fin[i]); break;
        case RCCE_PROD_FLOAT:  for (i=0; i<num; i++) fout[i] *= fin[i];             break;

        case RCCE_SUM_DOUBLE:  for (i=0; i<num; i++) dout[i] += din[i];             break;
        case RCCE_MAX_DOUBLE:  for (i=0; i<num; i++) dout[i] = MAX(dout[i],din[i]); break;
        case RCCE_MIN_DOUBLE:  for (i=0; i<num; i++) dout[i] = MIN(dout[i],din[i]); break;
        case RCCE_PROD_DOUBLE: for (i=0; i<num; i++) dout[i] *= din[i];             break;
      }
    }

    // in case of allreduce the root sends the reduction results to all non-root UEs
    if (all) for (ue=0; ue<comm.size; ue++) if (ue != root)
             if((ierr=RCCE_send(outbuf, num*type_size, comm.member[ue])))
                return(ierr);
  }
  return(RCCE_SUCCESS);
#endif // GORY
}

//---------------------------------------------------------------------------------------
// FUNCTION: RCCE_allreduce
//---------------------------------------------------------------------------------------
// Reduction function which delivers the reduction results to all participating UEs
//---------------------------------------------------------------------------------------
int RCCE_allreduce(
  char *inbuf,   // source buffer for reduction datan
  char *outbuf,  // target buffer for reduction data
  int num,       // number of data elements to be reduced
  int type,      // type of data elements
  int op,        // reduction operation
  RCCE_COMM comm // communication domain within which to reduce
  ){

  int root = 0, all = 1;
  return(RCCE_error_return(RCCE_debug_comm,
    RCCE_reduce_general(inbuf, outbuf, num, type, op, root, all, comm)));
}

//---------------------------------------------------------------------------------------
// FUNCTION: RCCE_reduce
//---------------------------------------------------------------------------------------
// Reduction function which delivers the reduction results to UE root
//---------------------------------------------------------------------------------------
int RCCE_reduce(
  char *inbuf,   // source buffer for reduction datan
  char *outbuf,  // target buffer for reduction data
  int num,       // number of data elements to be reduced
  int type,      // type of data elements
  int op,        // reduction operation
  int root,      // member of "comm" receiving reduction results
  RCCE_COMM comm // communication domain within which to reduce
  ){

  int ue, all = 0;
  // check to make sure root is member of the communicator
  if (root<0 || root >= comm.size) 
  return(RCCE_error_return(RCCE_debug_comm,RCCE_ERROR_ID));

  return(RCCE_error_return(RCCE_debug_comm,
      RCCE_reduce_general(inbuf, outbuf, num, type, op, root, all, comm)));
}

