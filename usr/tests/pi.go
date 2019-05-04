/*
 * Copyright (c) 2016, Stefan Lankes, RWTH Aachen University
 * All rights reserved.
 *
 * Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
 * http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
 * http://opensource.org/licenses/MIT>, at your option. This file may not be
 * copied, modified, or distributed except according to those terms.
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
