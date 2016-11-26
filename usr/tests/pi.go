/*
 * Copyright (c) 2016, Stefan Lankes, RWTH Aachen University
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

package main

import (
	"fmt"
	"os"
	"strconv"
	"time"
	"runtime"
)

var step float64

func term(ch chan float64, start, end int) {
	var res float64

	for i := start; i < end; i++ {
		x := (float64(i) + 0.5) * step
		res += 4.0 / (1.0 + x * x)
	}

	ch <- res
}

func main() {
	var num_steps int
	ch := make(chan float64)
	max_coroutines := runtime.NumCPU()

	if len(os.Args) > 1 {
		num_steps, _ = strconv.Atoi(os.Args[1])
	}
	if num_steps < 100 {
		num_steps = 1000000
	}
	fmt.Println("num_steps   : ", num_steps)

	sum := float64(0)
	step = 1.0 / float64(num_steps)

	start := time.Now()

	for i := 0; i < max_coroutines; i++ {
		start := (num_steps / max_coroutines) * i
		end := (num_steps / max_coroutines) * (i+1)

		go term(ch, start, end)
	}

	for i := 0; i < max_coroutines; i++ {
		sum += <-ch
	}

	elapsed := time.Since(start)

	fmt.Println("Pi          : ", sum*step)
	fmt.Println("Time        : ", elapsed)

	s := new(runtime.MemStats)
	runtime.ReadMemStats(s)

	fmt.Println("Alloc       : ", s.Alloc)
	fmt.Println("Total Alloc : ", s.TotalAlloc)
	fmt.Println("Sys         : ", s.Sys)
	fmt.Println("Lookups     : ", s.Lookups)
}
