#!/bin/sh

exec docker run -it --rm -v $(pwd):/mnt rust:12.04 /mnt/in-build.sh
