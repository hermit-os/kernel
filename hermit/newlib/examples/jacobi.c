/*
 * Copyright (c) 2010-2011, Stefan Lankes, RWTH Aachen University
 * All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions are met:
 *    * Redistributions of source code must retain the above copyright
 *      notice, this list of conditions and the following disclaimer.
 *    * Redistributions in binary form must reproduce the above copyright
 *      notice, this list of conditions and the following disclaimer in the
 *      documentation and/or other materials provided with the distribution.
 *    * Neither the name of the University nor the names of its contributors
 *      may be used to endorse or promote products derived from this
 *      software without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED
 * WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
 * DISCLAIMED. IN NO EVENT SHALL THE REGENTS OR CONTRIBUTORS BE LIABLE FOR ANY
 * DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES
 * (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES;
 * LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND
 * ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
 * (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS
 * SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <math.h>
#include <time.h>
#include <unistd.h>
#include <errno.h>

#define MATRIX_SIZE 	128
#define MAXVALUE	1337
#define PAGE_SIZE	4096
#define CACHE_SIZE	(256*1024)
#define ALIGN(x,a)	(((x)+(a)-1)&~((a)-1))

static int generate_empty_matrix(double*** A , unsigned int N) {
	unsigned int iCnt;
	int i,j;

	*A = (double**) malloc((N+1)*sizeof(double*));

	if (*A == NULL) 
		return -2;	/* Error */

	(*A)[0] = (double*) malloc((N+1)*N*sizeof(double));

	if (**A == NULL)
		return -2;	/* Error */

	for(iCnt=1; iCnt<N; iCnt++) { /* Assign pointers in the first "real index"; Value from 1 to N (0 yet set, value N means N+1) */
		(*A)[iCnt] = &((*A)[0][iCnt*(N+1)]);
	}

	memset(**A, 0, (N+1)*N*sizeof(double));      /* Fill matrix values with 0 */

	srand( 42 /*(unsigned) time(NULL)*/ ) ; /* init random number generator */

	/* 
	 * initialize the system of linear equations
	 * the result vector is one
	 */
	for (i = 0; i < N; i++) 
	{
		double sum = 0.0;

		for (j = 0; j < N; j++) 
		{
			if (i != j) 
			{
				double c = ((double)rand()) / ((double)RAND_MAX) * MAXVALUE;

				sum += fabs(c);
				(*A)[i][j] = c;
				(*A)[i][N] += c;
			}
		}

		/*
		 * The Jacobi method will always converge if the matrix A is strictly or irreducibly diagonally dominant. 
		 * Strict row diagonal dominance means that for each row, the absolute value of the diagonal term is 
		 * greater than the sum of absolute values of other terms: |A[i][i]| > Sum |A[i][j]| with (i != j)
		 */

		(*A)[i][i] = sum + 2.0;
		(*A)[i][N] += sum + 2.0;
	}

	return 0;
}

int main(int argc, char **argv)
{
	double*		temp;
	unsigned int	i, j, iter_start, iter_end;
	unsigned int	iterations = 0;
	double		error, norm, max = 0.0;
	double**	A=0;
	double*		X;
	double*		X_old, xi;
	//clock_t		start, end;

	if (generate_empty_matrix(&A,MATRIX_SIZE) < 0)
	{
		printf("generate_empty_matrix() failed...\n");
		exit(-1);

	}

	printf("generate_empty_matrix() done...\n");

	X = (double*) malloc(MATRIX_SIZE*sizeof(double));
	X_old = (double*) malloc(MATRIX_SIZE*sizeof(double));
	if(X == NULL || X_old == NULL)
	{
		printf("X or X_old is NULL...\n");
		exit(-1);
	}

	for(i=0; i<MATRIX_SIZE; i++) 
	{
		X[i] = ((double)rand()) / ((double)RAND_MAX) * 10.0;
		X_old[i] = 0.0;
	}

	printf("start calculation...\n");

	iter_start = 0;
	iter_end = MATRIX_SIZE;

	//start = clock();

	while(1) 
	{
		iterations++;
	
		temp = X_old;
		X_old = X;
		X = temp;

		for (i=iter_start; i<iter_end; i++) 
		{	
			for(j=0, xi=0.0; j<i; j++)
				xi += A[i][j] * X_old[j];

			for(j=i+1; j<MATRIX_SIZE; j++)
				xi += A[i][j] * X_old[j];
			X[i] = (A[i][MATRIX_SIZE] - xi) / A[i][i];
		}

		if (iterations % 5000 == 0 ) {/* calculate the Euclidean norm between X_old and X*/
			norm = 0.0;
			for (i=iter_start; i<iter_end; i++)
				norm += (X_old[i] - X[i]) * (X_old[i] - X[i]);

			/* check the break condition */
			norm /= (double) MATRIX_SIZE;		
			if (norm < 0.0000001)
				break;
		}
	}

	//end = clock();
	
	if (MATRIX_SIZE < 16) {
		printf("Print the solution...\n");
		/* print solution */
		for(i=0; i<MATRIX_SIZE; i++) {
			for(j=0; j<MATRIX_SIZE; j++) 
				printf("%8.2f\t", A[i][j]);
			printf("*\t%8.2f\t=\t%8.2f\n", X[i], A[i][MATRIX_SIZE]);
		}
	}
	printf("Check the result...\n");

	/* 
	 * check the result 
	 * X[i] have to be 1
	 */
	for(i=0; i<MATRIX_SIZE; i++) {
		error = fabs(X[i] - 1.0f);

		if (max < error)
			max = error;
		if (error > 0.01f) {
			printf("Result is on position %d wrong (%f != 1.0)\n", i, X[i]);
			exit(1);
		}
	}
	printf("maximal error is %f\n", max);

	printf("\nmatrix size: %d x %d\n", MATRIX_SIZE, MATRIX_SIZE);
	printf("number of iterations: %d\n", iterations);
	//printf("calculation time: %f s\n", (float) (end-start) / (float) CLOCKS_PER_SEC);

	free((void*) X_old);
	free((void*) X);

	return 0;
}
