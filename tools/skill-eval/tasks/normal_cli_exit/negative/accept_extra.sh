#!/bin/sh
if [ "$#" -lt 2 ] || [ "$1" != greet ]; then
    printf '%s\n' 'usage: tool.sh greet <name>' >&2
    exit 2
fi
printf 'hello %s\n' "$2"
