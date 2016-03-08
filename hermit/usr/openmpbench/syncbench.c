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
#include "syncbench.h"

omp_lock_t lock;

int main(int argc, char **argv) {

    // Start Paraver tracing
#ifdef PARAVERTRACE
    Extrae_init();
#endif

    init(argc, argv);

    omp_init_lock(&lock);

    /* GENERATE REFERENCE TIME */
    reference("reference time 1", &refer);

    /* TEST PARALLEL REGION */
    benchmark("PARALLEL", &testpr);

    /* TEST FOR */
    benchmark("FOR", &testfor);

    /* TEST PARALLEL FOR */
    benchmark("PARALLEL FOR", &testpfor);

    /* TEST BARRIER */
    benchmark("BARRIER", &testbar);

    /* TEST SINGLE */
    benchmark("SINGLE", &testsing);

    /* TEST  CRITICAL*/
    benchmark("CRITICAL", &testcrit);

    /* TEST  LOCK/UNLOCK */
    benchmark("LOCK/UNLOCK", &testlock);

    /* TEST ORDERED SECTION */
    benchmark("ORDERED", &testorder);

    /* GENERATE NEW REFERENCE TIME */
    reference("reference time 2", &referatom);

    /* TEST ATOMIC */
    benchmark("ATOMIC", &testatom);

    /* GENERATE NEW REFERENCE TIME */
    reference("reference time 3", &referred);

    /* TEST REDUCTION (1 var)  */
    benchmark("REDUCTION", &testred);

#ifdef PARAVERTRACE
    Extrae_fini();
#endif

    finalise();

    return EXIT_SUCCESS;
}

void refer() {
    int j;
    for (j = 0; j < innerreps; j++) {
	delay(delaylength);
    }
}

void referatom(){
    int j;
    double aaaa = 0.0;
    double epsilon = 1.0e-15;
    double b, c;
    b = 1.0;
    c = (1.0 + epsilon);
    for (j = 0; j < innerreps; j++) {
	aaaa += b;
	b *= c;
    }
    if (aaaa < 0.0)
	printf("%f\n", aaaa);
}

void referred() {
    int j;
    int aaaa = 0;
    for (j = 0; j < innerreps; j++) {
	delay(delaylength);
	aaaa += 1;
    }
}

void testpr() {
    int j;
    for (j = 0; j < innerreps; j++) {
#pragma omp parallel
	{
	    delay(delaylength);
	}
    }
}

void testfor() {
    int i, j;
#pragma omp parallel private(j)
    {
	for (j = 0; j < innerreps; j++) {
#pragma omp for
	    for (i = 0; i < nthreads; i++) {
		delay(delaylength);
	    }
	}
    }
}

void testpfor() {
    int i, j;
    for (j = 0; j < innerreps; j++) {
#pragma omp parallel for
	for (i = 0; i < nthreads; i++) {
	    delay(delaylength);
	}
    }
}

void testbar() {
    int j;
#pragma omp parallel private(j)
    {
	for (j = 0; j < innerreps; j++) {
	    delay(delaylength);
#pragma omp barrier
	}
    }
}

void testsing() {
    int j;
#pragma omp parallel private(j)
    {
	for (j = 0; j < innerreps; j++) {
#pragma omp single
	    delay(delaylength);
	}
    }
}

void testcrit() {
    int j;
#pragma omp parallel private(j)
    {
	for (j = 0; j < innerreps / nthreads; j++) {
#pragma omp critical
	    {
		delay(delaylength);
	    }
	}
    }
}

void testlock() {
    int j;

#pragma omp parallel private(j)
    {
	for (j = 0; j < innerreps / nthreads; j++) {
	    omp_set_lock(&lock);
	    delay(delaylength);
	    omp_unset_lock(&lock);
	}
    }
}

void testorder() {
    int j;
#pragma omp parallel for ordered schedule (static,1)
    for (j = 0; j < (int)innerreps; j++) {
#pragma omp ordered
	delay(delaylength);
    }
}

void testatom() {
    int j;
    double aaaa = 0.0;
    double epsilon = 1.0e-15;
    double b,c;
    b = 1.0;
    c = (1.0 + epsilon);
#pragma omp parallel private(j) firstprivate(b)
    {
	for (j = 0; j < innerreps / nthreads; j++) {
#pragma omp atomic	
	    aaaa += b;
	    b *= c;
	}
    }
    if (aaaa < 0.0)
	printf("%f\n", aaaa);
}

void testred() {
    int j;
    int aaaa = 0;
    for (j = 0; j < innerreps; j++) {
#pragma omp parallel reduction(+:aaaa)
	{
	    delay(delaylength);
	    aaaa += 1;
	}
    }
}

