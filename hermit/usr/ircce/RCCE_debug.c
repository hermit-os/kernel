//***************************************************************************************
// Diagnostic routines. 
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
#include "RCCE_lib.h"
#include "RCCE_debug.h"

#define MAX_ERROR_NUMBER 26

//......................................................................................
// GLOBAL VARIABLES USED BY THE LIBRARY
//......................................................................................
const char *RCCE_estrings[] = {
/*  0 */ "Success",
/*  1 */ "Invalid target buffer",
/*  2 */ "Invalid source buffer",
/*  3 */ "Invalid UE ID",
/*  4 */ "Invalid message length",
/*  5 */ "Flag variable undefined",
/*  6 */ "Emulated NUEs do not match requested NUEs",
/*  7 */ "Message buffers overlap in comm buffer",
/*  8 */ "Data buffer misalignment",
/*  9 */ "Debug flag not defined",
/* 10 */ "RCCE_flag variable not inside comm buffer",
/* 11 */ "Flag status not defined",
/* 12 */ "Flag not allocated",
/* 13 */ "Value not defined",
/* 14 */ "Invalid error code",
/* 15 */ "RPC data structure not allocated",
/* 16 */ "RPC internal error",
/* 17 */ "Multiple outstanding RPC requests",
/* 18 */ "Invalid power step",
/* 19 */ "Maximum allowable frequency exceeded",
/* 20 */ "No active RPC request",
/* 21 */ "Stale RPC request",
/* 22 */ "Communicator undefined",
/* 23 */ "Illegal reduction operator",
/* 24 */ "Illegal data type",
/* 25 */ "Memory allocation error",
/* 26 */ "Communicator initialization error",
/* 27 */ "Multicast is not supported in remote-put/local-get mode"
};
// GLOBAL VARIABLES USED BY THE LIBRARY

//--------------------------------------------------------------------------------------
// FUNCTION: RCCE_error_string
//--------------------------------------------------------------------------------------
// RCCE_error_string returns a descriptive error string   
//--------------------------------------------------------------------------------------
int RCCE_error_string(
  int err_no,         // number of error to be described
  char *error_string, // copy of error string
  int *string_length  // length of error string
  ) {
  
  if (err_no != RCCE_SUCCESS) {
    err_no -= RCCE_ERROR_BASE;
    if (err_no < 1 || err_no > MAX_ERROR_NUMBER) {
      strcpy(error_string,"");
      *string_length=0;
      return(RCCE_error_return(RCCE_debug_debug,RCCE_ERROR_INVALID_ERROR_CODE));
    }
  }
  strcpy(error_string,RCCE_estrings[err_no]);
  *string_length = strlen(RCCE_estrings[err_no]);
  return(RCCE_SUCCESS);
}

//--------------------------------------------------------------------------------------
// FUNCTION: RCCE_print_error
//--------------------------------------------------------------------------------------
// prints diagnostic error string, governed by input flag,  also returns the error code
//--------------------------------------------------------------------------------------
int RCCE_error_return(
  int debug_flag, // flag that controls diagnostic printing
  int err_no      // number of error to be printed
  ) {
  char error_string[RCCE_MAX_ERROR_STRING];
  int string_length;

  if (debug_flag && err_no) {
    RCCE_error_string(err_no, error_string, &string_length);
    fprintf(STDERR,"Error on UE %d: %s\n", RCCE_IAM, error_string); fflush(NULL);
  }
  return(err_no);
}
  

//--------------------------------------------------------------------------------------
// FUNCTION: RCCE_debug_set
//--------------------------------------------------------------------------------------
// turns on debugging of a certain library feature
//--------------------------------------------------------------------------------------
int RCCE_debug_set(
  int flag // flag that controls which library feaure is instrumented
  ){

  switch(flag) {
    case(RCCE_DEBUG_ALL):   RCCE_debug_synch=1;
                            RCCE_debug_comm=1;
                            RCCE_debug_debug=1;
                            RCCE_debug_RPC=1;
                            return(RCCE_SUCCESS);
    case(RCCE_DEBUG_SYNCH): RCCE_debug_synch=1;
                            return(RCCE_SUCCESS);
    case(RCCE_DEBUG_COMM):  RCCE_debug_comm=1;
                            return(RCCE_SUCCESS);
    case(RCCE_DEBUG_DEBUG): RCCE_debug_debug=1;
                            return(RCCE_SUCCESS);
    case(RCCE_DEBUG_RPC):   RCCE_debug_RPC=1;
                            return(RCCE_SUCCESS);
    default:                return(RCCE_error_return(RCCE_debug_debug,
                                                     RCCE_ERROR_DEBUG_FLAG));
  }
}

//--------------------------------------------------------------------------------------
// FUNCTION: RCCE_debug_unset
//--------------------------------------------------------------------------------------
// turns off debugging of a certain library feature
//--------------------------------------------------------------------------------------
int RCCE_debug_unset(
  int flag // flag that controls which library feaure is uninstrumented
  ){

  switch(flag) {
    case(RCCE_DEBUG_ALL):   RCCE_debug_synch=0;
                            RCCE_debug_comm=0;
                            RCCE_debug_debug=0;
                            RCCE_debug_RPC=0;
                            return(RCCE_SUCCESS);
    case(RCCE_DEBUG_SYNCH): RCCE_debug_synch=0;
                            return(RCCE_SUCCESS);
    case(RCCE_DEBUG_COMM):  RCCE_debug_comm=0;
                            return(RCCE_SUCCESS);
    case(RCCE_DEBUG_DEBUG): RCCE_debug_debug=0;
                            return(RCCE_SUCCESS);
    case(RCCE_DEBUG_RPC):   RCCE_debug_RPC=0;
                            return(RCCE_SUCCESS);
    default:                return(RCCE_error_return(RCCE_debug_debug,
                                                     RCCE_ERROR_DEBUG_FLAG));
  } 
}
