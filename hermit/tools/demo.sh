#!/bin/bash

# do not use this script
# it is written only for internal reasons

MYPROMPT="/work/HermitCore> "

clear
echo -n $MYPROMPT
echo " # HermitCore extends the multi-kernel approach and combines it with unikernel" | randtype -m 2 -t 18,10000
echo -n $MYPROMPT
echo " # features while providing better programmability and scalability for " | randtype -m 2 -t 18,10000
echo -n $MYPROMPT
echo " # hierarchical systems." | randtype -m 0 -t 18,10000
echo -n $MYPROMPT
echo ""
echo -n $MYPROMPT
echo " # By starting a HermitCore application, cores are be sperated from Linux and" | randtype -m 2 -t 18,10000
echo -n $MYPROMPT
echo " # a unikernel is be booted on these cores with the application." | randtype -m 2 -t 18,10000
echo -n $MYPROMPT
echo " # Consequently, HermitCore is a single-address space operating system which" | randtype -m 2 -t 18,10000
echo -n $MYPROMPT
echo " # promise a lower OS noise and better scalability." | randtype -m 1 -t 18,10000
echo -n $MYPROMPT
echo ""
echo -n $MYPROMPT
echo " # Now, a quick test via HelloWorld" | randtype -m 0 -t 18,10000
echo -n $MYPROMPT
echo " hermit/usr/tests/hello" | randtype -m 0 -t 18,10000
hermit/usr/tests/hello
echo -n $MYPROMPT
echo " # Linux' kernels messages show that CPU 1 is unplugged from Linux." | randtype -m 2 -t 18,10000
echo -n $MYPROMPT
echo " # After the termination of the HermitCore application, CPU 1 is" | randtype -m 1 -t 18,10000
echo -n $MYPROMPT
echo " # re-registered to the Linux system." | randtype -m 0 -t 18,10000
echo -n $MYPROMPT
echo " dmesg | tail -10" | randtype -m 1 -t 18,10000
dmesg | tail -10
echo -n $MYPROMPT
echo " # HermitCore's kernel message is published at /sys/hermit/isle0/log" | randtype -m 1 -t 18,10000
echo -n $MYPROMPT
echo " cat /sys/hermit/isle0/log" | randtype -m 1 -t 18,10000
cat /sys/hermit/isle0/log
echo -n $MYPROMPT
echo " # HermitCore supports OpenMP (including Intel's OpenMP Runtime)." | randtype -m 1 -t 18,10000
echo -n $MYPROMPT
echo " # The benchmark STREAM is used to show the mode of operation." | randtype -m 1 -t 18,10000
echo -n $MYPROMPT
echo " hermit/usr/benchmarks/stream" | randtype -m 0 -t 18,10000
hermit/usr/benchmarks/stream
echo -n $MYPROMPT
echo " # Per default, only CPU 1 is used. This can be changed by setting" | randtype -m 1 -t 18,10000
echo -n $MYPROMPT
echo " # the environment variable HERMIT_CPUS." | randtype -m 1 -t 18,10000
echo -n $MYPROMPT
echo " HERMIT_CPUS=\"1-2\" hermit/usr/benchmarks/stream" | randtype -m 0 -t 18,10000
HERMIT_CPUS="1-2" hermit/usr/benchmarks/stream
echo -n $MYPROMPT
echo " # In this example CPUs 1-2 are booted to run STREAM." | randtype -m 1 -t 18,10000
#echo -n $MYPROMPT
#echo " # Now, the same benchmark on Linux." | randtype -m 1 -t 18,10000
#echo -n $MYPROMPT
#echo " gcc -o stream_linux -O3 -fopenmp -mtune=native -march=native hermit/usr/benchmarks/stream.c" | randtype -m 0 -t 18,10000
#gcc -o stream_linux -O3 -fopenmp -mtune=native -march=native hermit/usr/benchmarks/stream.c
#echo -n $MYPROMPT
#echo " OMP_NUM_THREADS=2 ./stream_linux" | randtype -m 0 -t 18,10000
#OMP_NUM_THREADS=2 ./stream_linux
echo $MYPROMPT
echo -n $MYPROMPT
echo " # Our test system is a NUMA system based on Intel's CPU E5-2650 v3" | randtype -m 1 -t 18,10000
echo -n $MYPROMPT
echo " numactl --hardware" | randtype -m 0 -t 18,10000
numactl --hardware
echo -n $MYPROMPT
sleep 1
echo " lscpu" | randtype -m 0 -t 18,10000
lscpu
echo -n $MYPROMPT
echo " # HermitCore is able to boot an application on each NUMA node which we calle isles." | randtype -m 1 -t 18,10000
echo -n $MYPROMPT
echo " # The message passing interface iRCCE is supported for the inter-node communication." | randtype -m 1 -t 18,10000
echo -n $MYPROMPT
echo " # MPI support will be published soon. A prototyp exists already." | randtype -m 1 -t 18,10000
echo -n $MYPROMPT
echo " # The environment variable HERMIT_ISLE specifies on which isle the application will be started." | randtype -m 1 -t 18,10000
echo -n $MYPROMPT
echo " # The functionality will be demonstrated with a PingPong benchmark." | randtype -m 1 -t 18,10000
echo -n $MYPROMPT
echo " HERMIT_CPUS=1 HERMIT_ISLE=0 hermit/usr/benchmarks/RCCE_pingpong 2 533 0 1 &" | randtype -m 0 -t 18,10000
HERMIT_CPUS=1 HERMIT_ISLE=0 hermit/usr/benchmarks/RCCE_pingpong 2 533 0 1 &
sleep .3
echo -n $MYPROMPT
echo " HERMIT_CPUS=11 HERMIT_ISLE=1 hermit/usr/benchmarks/RCCE_pingpong 2 533 0 1" | randtype -m 1 -t 18,10000
HERMIT_CPUS=11 HERMIT_ISLE=1 hermit/usr/benchmarks/RCCE_pingpong 2 533 0 1 &
sleep 4
#echo $MYPROMPT
echo -n $MYPROMPT
sleep .3
echo " HermitCore (http://www.hermitcore.org) is an experimental platform. But try it out and send us a feedback!" | randtype -m 1 -t 18,10000
