#!/bin/bash
#
# do not use this script
# it is written only for internal tests via Travis CI

FILES="usr/tests/hello usr/tests/hellof usr/tests/hello++ usr/tests/thr_hello usr/tests/pi usr/benchmarks/stream usr/benchmarks/basic usr/tests/signals"
PROXY=/opt/hermit/bin/proxy

for f in $FILES; do echo "check $f..."; timeout --kill-after=5m 5m $PROXY $f || exit 1; done

# test echo server at port 8000
HERMIT_APP_PORT=8000 $PROXY usr/tests/server &
sleep 10
curl http://127.0.0.1:8000/help
sleep 1
curl http://127.0.0.1:8000/hello
sleep 1

# kill server
kill $!
