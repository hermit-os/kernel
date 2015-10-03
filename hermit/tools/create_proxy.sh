#!/bin/bash

if test "$#" -ne 1; then
  echo "Illegal number of parameters"
else
  fname=$(basename "$1")
  fname_new=$(basename "$1")_proxy

  echo "Create proxy for $fname"
  echo ".section .rodata" > inc.S
  echo ".global hermit_app" >> inc.S
  echo ".type   hermit_app, @object" >> inc.S
  echo ".align  4" >> inc.S
  echo "hermit_app:" >> inc.S
  echo .incbin \""$1"\" >> inc.S
  echo ".global app_size" >> inc.S
  echo ".type   app_size, @object" >> inc.S
  echo ".align  4" >> inc.S
  echo "app_size:" >> inc.S
  echo ".int    app_size - hermit_app" >> inc.S

  cc -O2 -Wall -o $fname_new proxy.c inc.S
fi
