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
#include <omp.h>

#include "common.h"
#include "taskbench.h"

#define DEPTH 6

int main(int argc, char **argv) {

    init(argc, argv);

#ifdef OMPVER3

    /* GENERATE REFERENCE TIME */
    reference("reference time 1", &refer);

    /* TEST PARALLEL TASK GENERATION */
    benchmark("PARALLEL TASK", &testParallelTaskGeneration);

    /* TEST MASTER TASK GENERATION */
    benchmark("MASTER TASK", &testMasterTaskGeneration);

    /* TEST MASTER TASK GENERATION WITH BUSY SLAVES */
    benchmark("MASTER TASK BUSY SLAVES", &testMasterTaskGenerationWithBusySlaves);

    /* TEST CONDITIONAL TASK GENERATION */
#ifndef DISABLE_CONDITIONAL_TASK_TEST
    benchmark("CONDITIONAL TASK", &testConditionalTaskGeneration);
#endif // DISABLE_CONDITIONAL_TASK_TEST

    /* TEST TASK WAIT */
    benchmark("TASK WAIT", &testTaskWait);

    /* TEST TASK BARRIER */
#ifndef DISABLE_BARRIER_TEST
    benchmark("TASK BARRIER", &testTaskBarrier);
#endif //DISABLE_BARRIER_TEST

#ifndef DISABLE_NESTED_TASKS_TESTS
    /* TEST NESTED TASK GENERATION */
    benchmark("NESTED TASK", &testNestedTaskGeneration);

    /* TEST NESTED MASTER TASK GENERATION */
    benchmark("NESTED MASTER TASK", &testNestedMasterTaskGeneration);

#endif // DISABLE_NESTED_TASKS_TESTS

    /* GENERATE THE SECOND REFERENCE TIME */
    reference("reference time 2", &refer);

    /* TEST BRANCH TASK TREE */
    benchmark("BRANCH TASK TREE", &testBranchTaskGeneration);

    /* TEST LEAF TASK TREE */
    benchmark("LEAF TASK TREE", &testLeafTaskGeneration);

#endif // OMPVER3

    finalise();

    return EXIT_SUCCESS;

}

/* Calculate the reference time. */
void refer() {
    int j;
    for (j = 0; j < innerreps; j++) {
	delay(delaylength);
    }

}

/* Calculate the second reference time. */
void refer2() {
    int j;
    for (j = 0; j < (innerreps >> DEPTH) * (1 << DEPTH); j++) {
	delay(delaylength);
    };

}

/* Test parallel task generation overhead */
void testParallelTaskGeneration() {
    int j;
#pragma omp parallel private( j )
    {
	for ( j = 0; j < innerreps; j ++ ) {
#pragma omp task
	    {
		delay( delaylength );

	    } // task
	}; // for j
    } // parallel

}

/* Test master task generation overhead */
void testMasterTaskGeneration() {
    int j;
#pragma omp parallel private(j)
    {
#pragma omp master
	{
	    /* Since this is executed by one thread we need innerreps * nthreads
	       iterations */
	    for (j = 0; j < innerreps * nthreads; j++) {
#pragma omp task
		{
		    delay(delaylength);

		}

	    } /* End for j */
	} /* End master */
    } /* End parallel */

}

/* Test master task generation overhead when the slave threads are busy */
void testMasterTaskGenerationWithBusySlaves() {
    int j;
#pragma omp parallel private( j )
    {
	int thread_num = omp_get_thread_num();
	for (j = 0; j < innerreps; j ++ ) {

	    if ( thread_num == 0 ) {
#pragma omp task
		{
		    delay( delaylength );
		} // task

	    } else {
		delay( delaylength );

	    }; // if
	}; // for j
    } // parallel
}

/* Measure overhead of checking if a task should be spawned. */
void testConditionalTaskGeneration() {
    int j;
#pragma omp parallel private(j)
    {
	for (j = 0; j < innerreps; j++) {
#pragma omp task if(returnfalse())
	    {
		delay( delaylength );
	    }
	}
    }
}

#ifndef DISABLE_NESTED_TASKS_TESTS

/* Measure overhead of nested tasks (all threads construct outer tasks) */
void testNestedTaskGeneration() {
    int i,j;
#pragma omp parallel private( i, j )
    {
	for ( j = 0; j < innerreps / nthreads; j ++ ) {
#pragma omp task private( i )
	    {
		for ( i = 0; i < nthreads; i ++ ) {
#pragma omp task untied
		    {
			delay( delaylength );

		    } // task
		}; // for i

		// wait for inner tasks to complete
#pragma omp taskwait

	    } // task
	}; // for j
    } // parallel
}

/* Measure overhead of nested tasks (master thread constructs outer tasks) */
void testNestedMasterTaskGeneration() {
    int i, j;
#pragma omp parallel private( i, j )
    {
#pragma omp master
	{
	    for ( j = 0; j < innerreps; j ++ ) {
#pragma omp task private( i )
		{
		    for ( i = 0; i < nthreads; i ++ ) {
#pragma omp task
			{
			    delay( delaylength );

			} // task
		    }; // for i

		    // wait for inner tasks to complete
#pragma omp taskwait

		} // task
	    }; // for j
	} // master
    } // parallel
}
#endif // DISABLE_NESTED_TASKS_TESTS

/* Measure overhead of taskwait (all threads construct tasks) */
void testTaskWait() {
    int j;
#pragma omp parallel private( j )
    {
	for ( j = 0; j < innerreps; j ++ ) {
#pragma omp task
	    {
		delay( delaylength );

	    } // task
#pragma omp taskwait

	}; // for j
    } // parallel
}

/* Measure overhead of tasking barrier (all threads construct tasks) */
void testTaskBarrier() {
    int j;
#pragma omp parallel private( j )
    {
	for ( j = 0; j < innerreps; j ++ ) {
#pragma omp task
	    {
		delay( delaylength );

	    } // task
#pragma omp barrier

	}; // for j
    } // parallel
}

/* Test parallel task generation overhead where work is done at all levels. */
void testBranchTaskGeneration() {
    int j;
#pragma omp parallel private(j)
    {
	for (j = 0; j < (innerreps >> DEPTH); j++) {
#pragma omp task
	    {
		branchTaskTree(DEPTH);
		delay(delaylength);
	    }

	}
    }
}

void branchTaskTree(int tree_level) {
    if ( tree_level > 0 ) {
#pragma omp task
	{
	    branchTaskTree(tree_level - 1);
	    branchTaskTree(tree_level - 1);
	    delay(delaylength);
	}
    }
}

/* Test parallel task generation overhead where work is done only at the leaf level. */
void testLeafTaskGeneration() {
    int j;
#pragma omp parallel private(j)
    {
	for (j = 0; j < (innerreps >> DEPTH); j++) {
	    leafTaskTree(DEPTH);

	}
    }

}

void leafTaskTree(int tree_level) {
    if ( tree_level == 0 ) {
	delay(delaylength);

    } else {
#pragma omp task
	{
	    leafTaskTree(tree_level - 1);
	    leafTaskTree(tree_level - 1);
	}
    }
}

