#!/bin/sh
find "$1" -name '*.txt' | while read path; do
    printf '%s\n' ${path##*/}
done
