
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
#include <stdlib.h>
#include <stdio.h>

#include "RCCE.h"

#undef _SCCPRAM_PUT_REMOTE_

extern t_vcharp RCCE_malloc(size_t);
extern int RCCE_put(t_vcharp, t_vcharp, int, int);
extern int RCCE_get(t_vcharp, t_vcharp, int, int);
extern int RCCE_acquire_lock(int);
extern int RCCE_release_lock(int);


#define MAX_STEPS 1000*10120
#define OCCURRENCE 100

#undef _TOKEN_PASSING_
#undef _MASTER_CORE_
#undef _MASTER_TREE_

#pragma omp thread_private(stamp_t, nuM_ranks, my_arnk, steps)
#pragma omp thread_private(my_token, global_tokens, remote_token)
#pragma omp thread_private(distance, neighbor, val_array)

typedef long long int stamp_t;

typedef struct _token_t
{
  stamp_t steps;
  int     rank;
  int     exit;
} token_t;


int num_ranks;
int my_rank;
stamp_t steps;

token_t my_token;
token_t *global_tokens;

int distance = 1;
int neighbor;

volatile unsigned char val_array[RCCE_LINE_SIZE];
token_t* remote_token;

#ifdef _TOKEN_PASSING_
void stop_and_check()
{ 
#ifndef _SCCPRAM_PUT_REMOTE_
  neighbor = RCCE_ue() - distance;
  if(neighbor < 0) neighbor = neighbor + RCCE_num_ues();
#else
  neighbor = RCCE_ue() + distance;
  if( neighbor > RCCE_num_ues() - 1 ) neighbor = neighbor - RCCE_num_ues();
#endif
  
  if(neighbor != RCCE_ue())
  {
    // do the token-passing:
    while(1)
    {
#ifndef _SCCPRAM_PUT_REMOTE_
      RCCE_get(val_array, (t_vcharp)global_tokens, RCCE_LINE_SIZE, neighbor);
#else
      RCCE_get(val_array, (t_vcharp)global_tokens, RCCE_LINE_SIZE, RCCE_ue() );
#endif
      
      remote_token = (token_t*)val_array;
      
      //printf("(%d|%lld) neighbor: %d|%lld / my_token: %d|%lld\n", RCCE_ue(), steps, remote_token->rank,  remote_token->steps, my_token.rank, my_token.steps);
      
      if(remote_token->exit)
      {
	int current = neighbor;
	// my current neighbor is atually finished --> determine a new neighbor:
	distance++;
	
#ifndef _SCCPRAM_PUT_REMOTE_
	neighbor = RCCE_ue() - distance;	
	if(neighbor < 0) neighbor = neighbor + RCCE_num_ues();
#else
	neighbor = RCCE_ue() + distance;
	if( neighbor > RCCE_num_ues() - 1 ) neighbor = neighbor - RCCE_num_ues();
#endif
	if(neighbor == RCCE_ue())
	{
	  // there is no other neighbor still running --> go on!
	  break;
	}
	else
	{
	  // restart token-passing with new neighbor...
	  continue;
	}
      }
      
      if(remote_token->rank == RCCE_ue())
      {
	// my own rank has rounded:
	if(remote_token->steps == steps)
	{
	  // go on!
	  break;
	}
	else
	{
	  // update my token:
	  my_token.steps = steps;
	  my_token.rank = RCCE_ue();
	  
#ifndef _SCCPRAM_PUT_REMOTE_
	  RCCE_put((t_vcharp)global_tokens, (t_vcharp)&my_token, RCCE_LINE_SIZE, RCCE_ue());
#else
	  RCCE_put((t_vcharp)global_tokens, (t_vcharp)&my_token, RCCE_LINE_SIZE, neighbor);
#endif
	}
      }
      else
      {
	if( (steps < remote_token->steps) || ((steps == remote_token->steps) && (RCCE_ue() < remote_token->rank)) )
	{
	  // update my token:
	  my_token.steps = steps;
	  my_token.rank = RCCE_ue();
	  
#ifndef _SCCPRAM_PUT_REMOTE_
	  RCCE_put((t_vcharp)global_tokens, (t_vcharp)&my_token, RCCE_LINE_SIZE, RCCE_ue());
#else
	  RCCE_put((t_vcharp)global_tokens, (t_vcharp)&my_token, RCCE_LINE_SIZE, neighbor);
#endif
	}
	else
	{
	  if( (remote_token->rank != my_token.rank) || (remote_token->steps != my_token.steps) )
	  {
	    // forward remote token:
	    memcpy(&my_token, remote_token, sizeof(token_t));
#ifndef _SCCPRAM_PUT_REMOTE_
	    RCCE_put((t_vcharp)global_tokens, (t_vcharp)&my_token, RCCE_LINE_SIZE, RCCE_ue());
#else
	    RCCE_put((t_vcharp)global_tokens, (t_vcharp)&my_token, RCCE_LINE_SIZE, neighbor);
#endif
	  }
	}
      }	
    }
  }
}
#else
#if defined(_MASTER_CORE_) || defined(_MASTER_TREE_)
void stop_and_check()
{
  while(1)
  {
    RCCE_get(val_array, (t_vcharp)global_tokens, RCCE_LINE_SIZE, RCCE_ue());
    remote_token = (token_t*)val_array;

    //    printf("RANK %d: %lld\n", my_rank, steps); fflush(stdout); sleep(1); 

    if(remote_token->exit) break;    
  }
}
#else
void stop_and_check()
{
  int i;
  int winner_rank = my_rank;
  stamp_t winner_steps = steps;

  while(1)
  {
    winner_rank = my_rank;
    winner_steps = steps;

    for(i=0; i<num_ranks; i++)
    {
      RCCE_get(val_array, (t_vcharp)global_tokens, RCCE_LINE_SIZE, i);
      remote_token = (token_t*)val_array;

      if(remote_token->steps < winner_steps) 
      {
	winner_steps = remote_token->steps;
	winner_rank  = remote_token->rank;
      }
    }

    if(winner_rank == my_rank) break;
  }
}
#endif
#endif

#ifdef _MASTER_CORE_
void one_single_master()
{
  int i, j;
  int winner_rank;
  int winner_flag;
  stamp_t winner_steps;
  int count;
  
  count = 0;

  while(1)
  {  
    winner_flag = 0;

    for(i=0,j=0; i<num_ranks - 1; i++)
    {
      RCCE_get(val_array, (t_vcharp)global_tokens, RCCE_LINE_SIZE, i);
      remote_token = (token_t*)val_array;

      if( (!j) || (remote_token->steps < winner_steps) )
      {
	if(remote_token->exit == 0)
	{
	  j=1;
	  winner_steps = remote_token->steps;
	  winner_rank  = remote_token->rank;
	  winner_flag  = 1;
	}
      }
    }
    
    if(winner_flag)
    {
      my_token.steps = winner_steps;
      my_token.rank  = winner_rank;
      my_token.exit  = 1;    

      RCCE_put((t_vcharp)global_tokens, (t_vcharp)&my_token, RCCE_LINE_SIZE, winner_rank);
    }

    count++;
    if(count == 1000)
    {
      RCCE_get(val_array, (t_vcharp)global_tokens, RCCE_LINE_SIZE, num_ranks-1);
      remote_token = (token_t*)val_array;
      
      if(remote_token->exit == num_ranks - 1) break;

      count = 0;
    }
  }
}
#endif
#ifdef _MASTER_TREE_
void multiple_masters(int flag)
{
  int i;
  int winner_rank;
  int winner_flag;
  stamp_t winner_steps;
  int start_rank;
  int num_ranks;
  int count;

  if(flag)
  {
    start_rank = 32 + (my_rank-40) * 2;
    num_ranks  = 2;
  }
  else
  {
    start_rank = (my_rank - 32) * 4;
    num_ranks  = 4;
  }
  
  RCCE_put((t_vcharp)global_tokens, (t_vcharp)&my_token, RCCE_LINE_SIZE, RCCE_ue()); 

  count = 0;
  
  while(1)
  {
    winner_flag = 0;

    for(i=start_rank; i<start_rank + num_ranks; i++)
    {
      RCCE_get(val_array, (t_vcharp)global_tokens, RCCE_LINE_SIZE, i);
      remote_token = (token_t*)val_array;

      if( (i==start_rank) || (remote_token->steps < winner_steps) )
      {
	winner_steps = remote_token->steps;
	winner_rank  = remote_token->rank;
	winner_flag  = 1;     
      }
    }
    
    if(winner_flag)
    {
      my_token.steps = winner_steps;
      my_token.rank  = winner_rank;    
     
      RCCE_put((t_vcharp)global_tokens, (t_vcharp)&my_token, RCCE_LINE_SIZE, RCCE_ue());
    }

    count++;
    if(count == 1000)
    {
      RCCE_get(val_array, (t_vcharp)global_tokens, RCCE_LINE_SIZE, 47);
      remote_token = (token_t*)val_array;
      
      if(remote_token->exit >= 32) break;

      count = 0;
    }
  }
}

void super_master()
{
  int i;
  int winner_rank;
  int winner_flag;
  stamp_t winner_steps;
  int start_rank;
  int count;
  int old_winner_rank = -1;
  stamp_t old_winner_steps = 0;
  
  RCCE_put((t_vcharp)global_tokens, (t_vcharp)&my_token, RCCE_LINE_SIZE, RCCE_ue()); 

  while(1)
  {
    winner_flag = 0;

    RCCE_get(val_array, (t_vcharp)global_tokens, RCCE_LINE_SIZE, 44);
    remote_token = (token_t*)val_array;

    winner_steps = remote_token->steps;
    winner_rank  = remote_token->rank;

    RCCE_get(val_array, (t_vcharp)global_tokens, RCCE_LINE_SIZE, 45);
    remote_token = (token_t*)val_array;

    if(remote_token->steps < winner_steps)
    {
      winner_steps = remote_token->steps;
      winner_rank  = remote_token->rank;
    }

    if( (winner_steps > old_winner_steps) || ( (winner_steps == old_winner_steps) && (winner_rank != old_winner_rank) ) )
    {
      winner_flag = 1;
      old_winner_rank  = winner_rank;
      old_winner_steps = winner_steps;
    }
      
    if( winner_flag && (winner_rank < 32) && (winner_rank % 2 == RCCE_ue() % 2) )
    {    
      my_token.steps = winner_steps;
      my_token.rank  = winner_rank;   
      my_token.exit  = 1;          
    
      RCCE_put((t_vcharp)global_tokens, (t_vcharp)&my_token, RCCE_LINE_SIZE, winner_rank);
    }

    count++;
    if(count == 1000)
    {
      RCCE_get(val_array, (t_vcharp)global_tokens, RCCE_LINE_SIZE, 47);
      remote_token = (token_t*)val_array;
      
      if(remote_token->exit >= 32) break;

      count = 0;
    }
  }
}
#endif

int RCCE_APP(int argc, char **argv)
{
  int i, j;
  double timer;

  RCCE_init(&argc, &argv);

  my_rank   = RCCE_ue();
  num_ranks = RCCE_num_ues();

  srand(my_rank);

  global_tokens = (token_t*)RCCE_malloc(RCCE_LINE_SIZE);
  
  my_token.steps = 0;
  my_token.exit  = 0;
  my_token.rank  = RCCE_ue();
  
  RCCE_put((t_vcharp)global_tokens, (t_vcharp)&my_token, RCCE_LINE_SIZE, RCCE_ue());   

  RCCE_barrier(&RCCE_COMM_WORLD);

#ifdef _TOKEN_PASSING_
  if(my_rank == 0) printf("MINIMUM started with %d procs and TOKEN PASSING ...\n", num_ranks); fflush(stdout);
#else
#ifdef _MASTER_CORE_
  if(my_rank == 0) printf("MINIMUM started with %d procs and MASTER CORE ...\n", num_ranks); fflush(stdout);
#else
#ifdef _MASTER_TREE_
  if(my_rank == 0) printf("MINIMUM started with %d procs and MASTER TREE ...\n", num_ranks); fflush(stdout);
#else
  if(my_rank == 0) printf("MINIMUM started with %d procs and GLOBAL VIEW ...\n", num_ranks); fflush(stdout);
#endif
#endif
#endif

  RCCE_barrier(&RCCE_COMM_WORLD);
    
  timer = RCCE_wtime() - timer;

#ifdef _MASTER_CORE_
  if(my_rank == RCCE_num_ues()-1) one_single_master();
  else for(steps=0; steps < MAX_STEPS / (num_ranks -1); steps++)
#else
#ifdef _MASTER_TREE_
  if(my_rank >= 32)
  {
    if(my_rank < 40) multiple_masters(0);
    else if(my_rank < 46) multiple_masters(1);
    else super_master();
  }
  else for(steps=0; steps < MAX_STEPS / 32; steps++)    
#else
  for(steps=0; steps < MAX_STEPS / num_ranks; steps++)
#endif
#endif
  {
    if( (rand() % OCCURRENCE) == 0 )
    {
      stop_and_check();
    }

    my_token.steps = steps;
    my_token.rank  = RCCE_ue();
    my_token.exit  = 0;

#ifndef _SCCPRAM_PUT_REMOTE_
    RCCE_put((t_vcharp)global_tokens, (t_vcharp)&my_token, RCCE_LINE_SIZE, RCCE_ue());
#else
    neighbor = RCCE_ue() + distance;
    if( neighbor > RCCE_num_ues() - 1 ) neighbor = neighbor - RCCE_num_ues();			  
    RCCE_put((t_vcharp)global_tokens, (t_vcharp)&my_token, RCCE_LINE_SIZE, neighbor);		
#endif
  }

#if defined(_MASTER_CORE_) || defined(_MASTER_TREE_)
  RCCE_acquire_lock(num_ranks-1);
  RCCE_get(val_array, (t_vcharp)global_tokens, RCCE_LINE_SIZE, num_ranks-1);
  remote_token = (token_t*)val_array;  
  remote_token->exit++;
  RCCE_put((t_vcharp)global_tokens, val_array, RCCE_LINE_SIZE, num_ranks-1);
  RCCE_release_lock(num_ranks-1);
#ifdef _MASTER_TREE_
  if(my_rank < 32)
  {
    my_token.steps = steps;
    my_token.rank  = RCCE_ue();
    my_token.exit  = 1;
    RCCE_put((t_vcharp)global_tokens, (t_vcharp)&my_token, RCCE_LINE_SIZE, RCCE_ue());
  }
#endif
#else
  my_token.steps = steps;
  my_token.rank  = RCCE_ue();
  my_token.exit  = 1;
  RCCE_put((t_vcharp)global_tokens, (t_vcharp)&my_token, RCCE_LINE_SIZE, RCCE_ue());
#endif

  RCCE_barrier(&RCCE_COMM_WORLD); 

  timer = RCCE_wtime() - timer;

  //printf("EXIT: %d at %lld / count: %d\n", my_rank, steps, remote_token->exit); fflush(stdout);
    
  if(my_rank == 0) printf("MINIMUM finished after %1.3lf sec.\n", timer); fflush(stdout);
  
  RCCE_finalize();


  return 0;
}

