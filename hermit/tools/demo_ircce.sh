#!/bin/bash

# do not use this script
# it is written only for internal reasons

MYPROMPT="~ > "

clear
echo -n $MYPROMPT
echo -e " \e[92m# HermitCore extends the multi-kernel approach and combines it with uni-\e[39m" | randtype -m 2 -t 18,6000
echo -n $MYPROMPT
echo -e " \e[92m# kernel features while providing better programmability and scalability\e[39m" | randtype -m 2 -t 18,6000
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
echo -e " \e[92m# Our test system is a NUMA system based on Intel's CPU E5-2650 v3\e[39m" | randtype -m 1 -t 18,6000
echo -n $MYPROMPT
echo " numactl --hardware" | randtype -m 0 -t 18,6000
numactl --hardware
echo -n $MYPROMPT
sleep 1
echo " lscpu" | randtype -m 0 -t 18,6000
lscpu
echo -n $MYPROMPT
echo -e " \e[92m# HermitCore is able to boot an application on each NUMA node which we call\e[39m" | randtype -m 1 -t 18,6000
echo -n $MYPROMPT
echo -e " \e[92m# isles. The message passing interface iRCCE is supported for the inter-node\e[39m" | randtype -m 1 -t 18,6000
echo -n $MYPROMPT
echo -e " \e[92m# communication. MPI support will be published soon. A prototyp exists\e[39m" | randtype -m 1 -t 18,6000
echo -n $MYPROMPT
echo -e " \e[92m# already. The environment variable HERMIT_ISLE specifies on which isle the\e[39m" | randtype -m 1 -t 18,6000
echo -n $MYPROMPT
echo -e " \e[92m# application will be started.\e[39m" | randtype -m 1 -t 18,6000
echo -n $MYPROMPT
echo -e " \e[92m# The functionality will be demonstrated with a PingPong benchmark.\e[39m" | randtype -m 1 -t 18,6000
echo -n $MYPROMPT
echo " HERMIT_CPUS=1 HERMIT_ISLE=0 hermit/usr/benchmarks/RCCE_pingpong 2 533 0 1 &" | randtype -m 0 -t 18,6000
HERMIT_CPUS=1 HERMIT_ISLE=0 hermit/usr/benchmarks/RCCE_pingpong 2 533 0 1 &
sleep .6
echo -n $MYPROMPT
echo " HERMIT_CPUS=11 HERMIT_ISLE=1 hermit/usr/benchmarks/RCCE_pingpong 2 533 0 1" | randtype -m 1 -t 18,6000
HERMIT_CPUS=11 HERMIT_ISLE=1 hermit/usr/benchmarks/RCCE_pingpong 2 533 0 1 &
sleep 4
#echo $MYPROMPT
echo -n $MYPROMPT
sleep .3
echo -e " \e[92mHermitCore (\e[31mhttp://www.hermitcore.org\e[92m) is an experimental platform.\e[39m" | randtype -m 1 -t 18,6000
echo -n $MYPROMPT
echo -e " \e[92mBut try it out and send us a feedback!\e[39m" | randtype -m 1 -t 18,6000
