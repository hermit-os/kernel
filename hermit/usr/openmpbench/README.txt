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

===============
 Licence
===============
This software is released under the licence in Licence.txt

===============
 Introduction
===============
Overheads due to synchronisation, loop scheduling, array operations and 
task scheduling are an important factor in determining the performance of 
shared memory parallel programs. We have designed a set of microbenchmarks 
to measure these classes of overhead for language constructs in OpenMP. 

===============
 Installation
===============
 1. Unpack the tar file

 2. Edit the Makefile.defs as follows:
    * Set CC to the C compiler you wish to use (e.g. gcc pgcc icc xlc etc)
    * Set CFLAGS to any required C compiler flags to enable processing of 
      OpenMP directives (e.g. -fopenmp -mp, -omp); standard optimisation is 
      also recommended (e.g. -O).
    * Set LDFLAGS to any required C linker flags
    * Set CPP to the local C-Preprocessor (e.g. /usr/local/bin/cpp) to 
      make the C compiler invoke cpp on .c and .h files
    * To benchmark OpenMP 2.0 features can be invoked by setting the flag 
	OMPFLAG = -DOMPVER2
    * To benchmark OpenMP 2.0 & 3.0 features can be invoked by setting the flag 
	OMPFLAG = -DOMPVER2 -DOMPVER3
    * If neither of these flags are set then OpenMP 1.0 compatibility is 
      ensured.

3. Type "make" to build all 4 benchmarks or "make benchmark" where benchmark 
    is one of syncbench, taskbench, schedbench. By default "make" will build 
    executables with array sizes ranging in powers of 3 from 1 to 59049. To 
    build the array benchmark with an array size of arraysize, use 
    "make IDA=arraysize prog" where arraysize is a positive integer. 


Example Makefile.defs.* files are supplied for several machines and
compiler versions, e.g. 
	 Makefile.defs.hector.* - Cray XE6 
	 Makefile.defs.magny0.* - 48 core AMD Magny Cours machine
	 Makefile.defs.stokes.*	- SGI Altix ICE 8200EX


===============
 Running
===============

1. Set OMP_NUM_THREADS to the number of OpenMP threads you want to run with, 
   e.g. export OMP_NUM_THREADS = 4
   OMP_NUM_THREADS should be less than or equal to the number of physical 
   cores available to you. 

2. Run the benchmark with:
   ./benchmark 

   The output will go to STDOUT and thus you will probably want to re-direct 
   this to a file. ./benchmark --help will give the usage options. 


=================
Additional notes
=================

 1. If you encounter problems with the value of innerreps becoming too 
    large (an error will be reported) try recompiling with a lower level of 
    optimisation, ideally with inlining turned off. 

 2. It is common to observe significant variability between the overhead 
    values obtained on different runs of the benchmark programs. Therefore, 
    it is advisable to run each benchmark, say, 10-20 times and average the 
    results obtained.

 3. You should use whatever mechanisms are at your disposal to ensure that 
    threads have exclusive or almost exclusive access to processors. You 
    should rejects runs where the standard deviation or number of outliers is 
    large: this is a good indication that the benchmark did not have almost 
    exclusive access to processors. 
