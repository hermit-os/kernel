#!/bin/bash
#
# do not use this script
# it is written only for internal tests via Travis CI

FILES="hermit/usr/tests/hello hermit/usr/tests/hellof hermit/usr/tests/hello++ hermit/usr/tests/thr_hello hermit/usr/tests/pi hermit/usr/benchmarks/stream hermit/usr/benchmarks/basic"
PROXY=hermit/tools/proxy

for f in $FILES; do echo "check $f..."; timeout 180 $PROXY $f || exit 1; done

# test echo server at port 8000
$PROXY hermit/usr/tests/server &
sleep 10
curl http://127.0.0.1:8000/help
sleep 1

# kill server
kill $!
