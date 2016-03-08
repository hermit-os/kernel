/****************************************************************************
*                                                                           *
*             OpenMP MicroBenchmark Suite - Version 3.1                     *
*                                                                           *
*                            produced by                                    *
*                                                                           *
*             Mark Bull, Fiona Reid and Nix Mc Donnell                      *
*                                                                           *
*                                at                                         *
*                                                                           *
*                Edinburgh Parallel Computing Centre                        *
*                                                                           *
*         email: markb@epcc.ed.ac.uk or fiona@epcc.ed.ac.uk                 *
*                                                                           *
*                                                                           *
*      This version copyright (c) The University of Edinburgh, 2015.        *
*                                                                           *
*                                                                           *
*  Licensed under the Apache License, Version 2.0 (the "License");          *
*  you may not use this file except in compliance with the License.         *
*  You may obtain a copy of the License at                                  *
*                                                                           *
*      http://www.apache.org/licenses/LICENSE-2.0                           *
*                                                                           *
*  Unless required by applicable law or agreed to in writing, software      *
*  distributed under the License is distributed on an "AS IS" BASIS,        *
*  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied. *
*  See the License for the specific language governing permissions and      *
*  limitations under the License.                                           *
*                                                                           *
****************************************************************************/

#include <stdio.h>
#include <stdlib.h>
#include <math.h>
#include <omp.h>

#include "common.h"
#include "arraybench.h"

double btest[IDA];
double atest[IDA];

#pragma omp threadprivate (btest)

int main(int argc, char **argv) {

    init(argc, argv);

    /* GENERATE REFERENCE TIME */
    reference("reference time 1", &refer);

    char testName[32];

    /* TEST  PRIVATE */
    sprintf(testName, "PRIVATE %d", IDA);
    benchmark(testName, &testprivnew);

    /* TEST  FIRSTPRIVATE */
    sprintf(testName, "FIRSTPRIVATE %d", IDA);
    benchmark(testName, &testfirstprivnew);

#ifdef OMPVER2
    /* TEST  COPYPRIVATE */
    sprintf(testName, "COPYPRIVATE %d", IDA);
    benchmark(testName, &testcopyprivnew);
#endif

    /* TEST  THREADPRIVATE - COPYIN */
    sprintf(testName, "COPYIN %d", IDA);
    benchmark(testName, &testthrprivnew);

    finalise();

    return EXIT_SUCCESS;

}

void refer() {
    int j;
    double a[1];
    for (j = 0; j < innerreps; j++) {
	array_delay(delaylength, a);
    }
}

void testfirstprivnew() {
    int j;
    for (j = 0; j < innerreps; j++) {
#pragma omp parallel firstprivate(atest)
	{
	    array_delay(delaylength, atest);
	}
    }
}

void testprivnew() {
    int j;
    for (j = 0; j < innerreps; j++) {
#pragma omp parallel private(atest)
	{
	    array_delay(delaylength, atest);
	}
    }
}

#ifdef OMPVER2
void testcopyprivnew()
{
    int j;
    for (j=0; j<innerreps; j++) {
#pragma omp parallel private(atest)
	{
#pragma omp single copyprivate(atest)
		{
	    	array_delay(delaylength, atest);
		}
    	}
    }
}

#endif

void testthrprivnew() {
    int j;
    for (j = 0; j < innerreps; j++) {
#pragma omp parallel copyin(btest)
	{
	    array_delay(delaylength, btest);
	}
    }

}
