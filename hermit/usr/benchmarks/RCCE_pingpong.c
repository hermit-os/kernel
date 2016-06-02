
/* 
 * Copyright 2010 Carsten Clauss, Chair for Operating Systems,
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

#include <string.h>
#include <stdio.h>

#include "RCCE.h"

#define  _CACHE_WARM_UP_
#undef  _USE_SEPARATED_BUFFERS_
#undef  _ERROR_CHECK_

#define MAXBUFSIZE  1024*1024*64
#define DEFAULTLEN  1024*1024
#define NUMROUNDS   10000


#ifdef _USE_SEPARATED_BUFFERS_
static char send_buffer[MAXBUFSIZE+1] __attribute__ ((aligned (64)));
static char recv_buffer[MAXBUFSIZE+1] __attribute__ ((aligned (64)));
#else
#define send_buffer buffer
#define recv_buffer buffer
static char buffer[MAXBUFSIZE+1] __attribute__ ((aligned (64)));
#endif
static char dummy = 0;

int RCCE_APP(int argc, char **argv)
{
  int i;
  int num_ranks;
  int remote_rank, my_rank;
  int numrounds = NUMROUNDS;
  int maxlen    = DEFAULTLEN;
  int length;
  int round;
  int ircce_mode = 0;
  double timer;
  RCCE_SEND_REQUEST rcce_send_req;
  RCCE_RECV_REQUEST rcce_recv_req;

  RCCE_init(&argc, &argv);

  my_rank   = RCCE_ue();
  num_ranks = RCCE_num_ues();

  if(argc > 1) numrounds = atoi(argv[1]);

  if(numrounds < 1)
  {
    if(my_rank == 0) fprintf(stderr, "Pingpong needs at least 1 round; try again\n");
    exit(-1);
  }

  if(argc > 2) maxlen = atoi(argv[2]);

  if(maxlen < 1)
  {
    if(my_rank == 0) fprintf(stderr, "Illegal message size: %s; try again\n", argv[2]);
    exit(-1);
  }
  else if(maxlen > MAXBUFSIZE)
  {
    if(my_rank == 0) fprintf(stderr, "Message size %d is too big; try again\n", maxlen);
    exit(-1);  
  }

  if(argc > 3) ircce_mode = 1;

  if (ircce_mode && !my_rank)
    printf("Use iRCCE mode\n");

  if(num_ranks != 2)
  {
    if(my_rank == 0) fprintf(stderr, "Pingpong needs exactly two UEs; try again\n");
    exit(-1);
  
  }
  
  remote_rank = (my_rank + 1) % 2;

  if(my_rank == 0) printf("#bytes\t\tusec\t\tMB/sec\n");

  for(length=1; length <= maxlen; length*=2)
  {

#ifdef _CACHE_WARM_UP_
    for(i=0; i < length; i++)
    {
      /* cache warm-up: */
      dummy += send_buffer[i];
      dummy += recv_buffer[i];
    }
#endif

    /* synchronize before starting PING-PONG: */
    RCCE_barrier(&RCCE_COMM_WORLD);

    for(round=0; round < numrounds+1; round++)
    {

#ifdef _ERROR_CHECK_
      for(i=0; i < length; i++)
      {
	send_buffer[i] = (i+length+round) % 127;
      }
#endif

      if(my_rank == 0)
      {
	/* send PING: */
	if(ircce_mode) {
	  RCCE_isend(send_buffer, length, remote_rank, &rcce_send_req);
	  RCCE_isend_wait(&rcce_send_req);
	} else
	RCCE_send(send_buffer, length, remote_rank);

	/* recv PONG: */
	if(ircce_mode) {
	  RCCE_irecv(recv_buffer, length, remote_rank, &rcce_recv_req);
	  RCCE_irecv_wait(&rcce_recv_req);
	} else
	RCCE_recv(recv_buffer, length, remote_rank);
      } 
      else
      {
	/* recv PING: */
	if(ircce_mode) {
	  RCCE_irecv(recv_buffer, length, remote_rank, &rcce_recv_req);
	  RCCE_irecv_wait(&rcce_recv_req);
	} else
	RCCE_recv(recv_buffer, length, remote_rank);

	/* send PONG: */
	if(ircce_mode) {
	  RCCE_isend(send_buffer, length, remote_rank, &rcce_send_req);
	  RCCE_isend_wait(&rcce_send_req);
	} else
	RCCE_send(send_buffer, length, remote_rank);
      }

      /* start timer: */
      if(round == 0) timer = RCCE_wtime();

#ifdef _ERROR_CHECK_
      for(i=0; i < length; i++)
      {
	if(recv_buffer[i] != (i+length+round) % 127)
	{
	  fprintf(stderr, "ERROR: %d VS %d\n", recv_buffer[i], (i+length+round) % 127);
	  exit(-1);
	}
      }
#endif
    }
    
    /* stop timer: */
    timer = RCCE_wtime() - timer;
    
    if(my_rank == 0) printf("%d\t\t%1.2lf\t\t%1.2lf\n", length, timer/(2.0*numrounds)*1000000, (length / (timer/(2.0*numrounds))) / (1024*1024) );
    fflush(stdout);
  }

  RCCE_finalize();

  return 0;
}

