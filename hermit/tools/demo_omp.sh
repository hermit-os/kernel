#!/bin/bash

# do not use this script
# it is written only for internal reasons

MYPROMPT="~ > "

clear
echo -n $MYPROMPT
echo -e " \e[92m# HermitCore extends the multi-kernel approach and combines it with\e[39m" | randtype -m 2 -t 18,6000
echo -n $MYPROMPT
echo -e " \e[92m# unikernel features while providing better programmability and scalability\e[39m" | randtype -m 2 -t 18,6000
echo -n $MYPROMPT
echo -e " \e[92m# for hierarchical systems.\e[39m" | randtype -m 0 -t 18,6000
echo -n $MYPROMPT
echo ""
echo -n $MYPROMPT
echo -e " \e[92m# By starting a HermitCore application, cores are be sperated from Linux and\e[39m" | randtype -m 2 -t 18,6000
echo -n $MYPROMPT
echo -e " \e[92m# a unikernel is be booted on these cores with the application.\e[39m" | randtype -m 2 -t 18,6000
echo -n $MYPROMPT
echo -e " \e[92m# Consequently, HermitCore is a single-address space operating system which\e[39m" | randtype -m 2 -t 18,6000
echo -n $MYPROMPT
echo -e " \e[92m# promise a lower OS noise and better scalability.\e[39m" | randtype -m 1 -t 18,6000
echo -n $MYPROMPT
echo ""
echo -n $MYPROMPT
echo -e " \e[92m# Now, a quick test via HelloWorld\e[39m" | randtype -m 0 -t 18,6000
echo -n $MYPROMPT
echo  " hermit/usr/tests/hello" | randtype -m 0 -t 18,6000
hermit/usr/tests/hello
echo -n $MYPROMPT
echo -e " \e[92m# Linux' kernels messages show that CPU 1 is unplugged from Linux.\e[39m" | randtype -m 2 -t 18,6000
echo -n $MYPROMPT
echo -e " \e[92m# After the termination of the HermitCore application, CPU 1 is\e[39m" | randtype -m 1 -t 18,6000
echo -n $MYPROMPT
echo -e " \e[92m# re-registered to the Linux system.\e[39m" | randtype -m 0 -t 18,6000
echo -n $MYPROMPT
echo " dmesg | tail -10" | randtype -m 1 -t 18,6000
dmesg | tail -10
echo -n $MYPROMPT
echo -e " \e[92m# HermitCore's kernel message is published at /sys/hermit/isle0/log\e[39m" | randtype -m 1 -t 18,6000
echo -n $MYPROMPT
echo " cat /sys/hermit/isle0/log" | randtype -m 1 -t 18,6000
cat /sys/hermit/isle0/log
echo -n $MYPROMPT
echo -e " \e[92m# HermitCore supports OpenMP (including Intel's OpenMP Runtime).\e[39m" | randtype -m 1 -t 18,6000
echo -n $MYPROMPT
echo -e " \e[92m# The benchmark STREAM is used to show the mode of operation.\e[39m" | randtype -m 1 -t 18,6000
echo -n $MYPROMPT
echo " hermit/usr/benchmarks/stream" | randtype -m 0 -t 18,6000
hermit/usr/benchmarks/stream
echo -n $MYPROMPT
echo -e " \e[92m# Per default, only CPU 1 is used. This can be changed by setting\e[39m" | randtype -m 1 -t 18,6000
echo -n $MYPROMPT
echo -e " \e[92m# the environment variable HERMIT_CPUS.\e[39m" | randtype -m 1 -t 18,6000
echo -n $MYPROMPT
echo " HERMIT_CPUS=\"1-2\" hermit/usr/benchmarks/stream" | randtype -m 0 -t 18,6000
HERMIT_CPUS="1-2" hermit/usr/benchmarks/stream
echo -n $MYPROMPT
echo -e " \e[92m# In this example CPUs 1-2 are booted to run STREAM.\e[39m" | randtype -m 1 -t 18,6000
#echo -n $MYPROMPT
#echo -e " \e[92m# Now, the same benchmark on Linux.\e[39m" | randtype -m 1 -t 18,6000
#echo -n $MYPROMPT
#echo " gcc -o stream_linux -O3 -fopenmp -mtune=native -march=native hermit/usr/benchmarks/stream.c" | randtype -m 0 -t 18,6000
#gcc -o stream_linux -O3 -fopenmp -mtune=native -march=native hermit/usr/benchmarks/stream.c
#echo -n $MYPROMPT
#echo " OMP_NUM_THREADS=2 ./stream_linux" | randtype -m 0 -t 18,6000
#OMP_NUM_THREADS=2 ./stream_linux
echo $MYPROMPT
echo -n $MYPROMPT
echo -e " \e[92mHermitCore (\e[31mhttp://www.hermitcore.org\e[92m) is an experimental platform.\e[39m" | randtype -m 1 -t 18,6000
echo -n $MYPROMPT
echo -e " \e[92mBut try it out and send us a feedback!\e[39m" | randtype -m 1 -t 18,6000

