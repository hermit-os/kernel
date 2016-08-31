#!/bin/bash
#
# do not use this script
# it is written only for internal tests via Travis CI

FILES="hermit/usr/tests/hello hermit/usr/tests/hellof hermit/usr/tests/hello++ hermit/usr/tests/thr_hello hermit/usr/benchmarks/stream hermit/usr/benchmarks/basic"
PROXY=hermit/tools/proxy

for f in $FILES; do echo "check $f..."; $PROXY $f || exit 1; done
