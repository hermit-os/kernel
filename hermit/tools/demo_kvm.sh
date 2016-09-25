#!/bin/bash

# do not use this script
# it is written only for internal reasons

MYPROMPT="~ > "

clear
echo -n $MYPROMPT
echo -e " \e[92m# HermitCore is also usable as a classical unikernel. By setting the\e[39m" | randtype -m 2 -t 18,6000
echo -n $MYPROMPT
echo -e " \e[92m# environment variable HERMIT_ISLE to qemu, the application will be started\e[39m" | randtype -m 2 -t 18,6000
echo -n $MYPROMPT
echo -e " \e[92m# within a virtual machine. The boot time is about 1s.\e[39m" | randtype -m 0 -t 18,6000
echo -n $MYPROMPT
echo  " HERMIT_ISLE=qemu time hermit/usr/tests/hello" | randtype -m 0 -t 18,6000
HERMIT_ISLE=qemu time hermit/usr/tests/hello
echo -n $MYPROMPT
echo -e " \e[92m# The variable HERMIT_CPUS defines the number of virtual cores, while\e[39m" | randtype -m 2 -t 18,6000
echo -n $MYPROMPT
echo -e " \e[92m# HERMIT_MEM specifies the memory size of the virtual machine.\e[39m" | randtype -m 1 -t 18,6000
echo -n $MYPROMPT
echo " HERMIT_ISLE=qemu HERMIT_CPUS=4 HERMIT_MEM=1G hermit/usr/benchmarks/stream" | randtype -m 1 -t 18,6000
HERMIT_ISLE=qemu HERMIT_CPUS=4 HERMIT_MEM=1G hermit/usr/benchmarks/stream
echo -n $MYPROMPT
echo -e " \e[92m# HermitCore's kernel messages are published by setting the environment\e[39m" | randtype -m 1 -t 18,6000
echo -n $MYPROMPT
echo -e " \e[92m# HERMIT_VERBOSE to 1.\e[39m" | randtype -m 1 -t 18,6000
echo -n $MYPROMPT
echo " HERMIT_ISLE=qemu HERMIT_CPUS=4 HERMIT_VERBOSE=1 hermit/usr/benchmarks/stream" | randtype -m 1 -t 18,6000
HERMIT_ISLE=qemu HERMIT_CPUS=4 HERMIT_VERBOSE=1 hermit/usr/benchmarks/stream
#echo $MYPROMPT
echo -n $MYPROMPT
echo -e " \e[92mHermitCore (\e[31mhttp://www.hermitcore.org\e[92m) is an experimental platform.\e[39m" | randtype -m 1 -t 18,6000
echo -n $MYPROMPT
echo -e " \e[92mBut try it out and send us a feedback!\e[39m" | randtype -m 1 -t 18,6000

