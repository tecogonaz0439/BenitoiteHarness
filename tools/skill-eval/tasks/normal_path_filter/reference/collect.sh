#!/bin/sh
for path in "$1"/*.txt "$1"/.*.txt "$1"/.txt; do
    if [ -f "$path" ] && [ ! -L "$path" ]; then
        printf '%s\n' "${path##*/}"
    fi
done | LC_ALL=C sort
