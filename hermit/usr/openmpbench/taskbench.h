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

#ifndef TASKBENCH_H
#define TASKBENCH_H

void refer(void);

void refer2(void);

void stats(double*, double*);

void testParallelTaskGeneration(void);

void testMasterTaskGeneration(void);

void testMasterTaskGenerationWithBusySlaves(void);

void testNestedTaskGeneration(void);

void testNestedMasterTaskGeneration(void);

void testTaskWait(void);

void testTaskBarrier(void);

void testConditionalTaskGeneration(void);

void testBranchTaskGeneration(void);

void branchTaskTree(int tree_level);

void testLeafTaskGeneration(void);

void leafTaskTree(int tree_level);

#endif //TASKBENCH_H
