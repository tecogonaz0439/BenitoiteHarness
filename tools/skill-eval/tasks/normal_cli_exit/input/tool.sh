#!/bin/sh
if [ "$#" -ne 2 ] || [ "$1" != greet ]; then
    printf '%s\n' 'usage: tool.sh greet <name>'
    exit 0
fi
printf 'hello %s\n' $2
