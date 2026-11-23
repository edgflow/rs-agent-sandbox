#!/bin/sh
# Makes bridge/toolbox.img once: a data disk with Alpine's rust and cargo
# installed (about 40 seconds and 550 MB, the file itself is sparse).
# Every task then opens it in throw-away mode: writes stay in memory, so
# each task starts clean and the image never changes.
#
# This one-time setup uses the network (Alpine's package mirror). The agent's
# tasks never do, unless Rust allows it (Lesson 5).
set -e
cd "$(dirname "$0")"
if [ -e toolbox.img ]; then
  echo "toolbox.img already exists"
  exit 0
fi
node --experimental-wasm-jspi node_modules/.bin/unkernel \
  --data toolbox.img --timeout 300 \
  --input '{6}sudo apk add -q rust cargo && echo TOOLBOX-READY; sudo poweroff\n'
