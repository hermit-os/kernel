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
#include <xray.h>

#include "common.h"
#include "syncbench.h"

omp_lock_t lock;

int main(int argc, char **argv) {

	struct XRayTraceCapture* trace = XRayInit(
				20,					// max. call depth
				16 * 1000 * 1000,	// memory for report
				13,					// frame count
				"syncbench.map");

    // Start Paraver tracing
#ifdef PARAVERTRACE
    Extrae_init();
#endif

    init(argc, argv);

    omp_init_lock(&lock);

    /* GENERATE REFERENCE TIME */
	XRayStartFrame(trace);
	reference("reference time 1", &refer);
	XRayEndFrame(trace);

    /* TEST PARALLEL REGION */
	XRayStartFrame(trace);
    benchmark("PARALLEL", &testpr);
	XRayEndFrame(trace);

	/* TEST FOR */
	XRayStartFrame(trace);
	benchmark("FOR", &testfor);
	XRayEndFrame(trace);

	/* TEST PARALLEL FOR */
	XRayStartFrame(trace);
	benchmark("PARALLEL FOR", &testpfor);
	XRayEndFrame(trace);

	/* TEST BARRIER */
	XRayStartFrame(trace);
	benchmark("BARRIER", &testbar);
	XRayEndFrame(trace);

	/* TEST SINGLE */
	XRayStartFrame(trace);
	benchmark("SINGLE", &testsing);
	XRayEndFrame(trace);

	/* TEST  CRITICAL*/
	XRayStartFrame(trace);
	benchmark("CRITICAL", &testcrit);
	XRayEndFrame(trace);

	/* TEST  LOCK/UNLOCK */
	XRayStartFrame(trace);
	benchmark("LOCK/UNLOCK", &testlock);
	XRayEndFrame(trace);

	/* TEST ORDERED SECTION */
	XRayStartFrame(trace);
	benchmark("ORDERED", &testorder);
	XRayEndFrame(trace);

	/* GENERATE NEW REFERENCE TIME */
	XRayStartFrame(trace);
	reference("reference time 2", &referatom);
	XRayEndFrame(trace);

	/* TEST ATOMIC */
	XRayStartFrame(trace);
	benchmark("ATOMIC", &testatom);
	XRayEndFrame(trace);

	/* GENERATE NEW REFERENCE TIME */
	XRayStartFrame(trace);
	reference("reference time 3", &referred);
	XRayEndFrame(trace);

	/* TEST REDUCTION (1 var)  */
	XRayStartFrame(trace);
	benchmark("REDUCTION", &testred);
	XRayEndFrame(trace);

#ifdef PARAVERTRACE
    Extrae_fini();
#endif

	XRaySaveReport(trace,
				   "syncbench.xray", // report file
				   0.05f, // Only output funcs that have higher runtime [%]
				   1000); // Only output funcs that have higher runtime [cycles]
	XRayShutdown(trace);

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
#ifdef XRAY
	static int n = 1;
	XRayAnnotate("n = %i", n);
	n++;
#endif
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
#ifdef XRAY
	static int n = 1;
	XRayAnnotate("n = %i", n);
	n++;
#endif
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

