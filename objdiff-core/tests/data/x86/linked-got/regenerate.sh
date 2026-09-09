#!/bin/sh
set -eu
cd "$(dirname "$0")"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
for side in left right; do
    flag=0
    [ "$side" = left ] || flag=1
    as --32 -mrelax-relocations=no --defsym RIGHT="$flag" -o "$tmp/$side.o" fixture.s
    ld -m elf_i386 -shared -Bsymbolic --build-id=none -o "$side.elf" "$tmp/$side.o"
done
# Retain an ordinary relocation fixture to assert recovery does not alter ET_REL.
cp "$tmp/left.o" relocatable.o
