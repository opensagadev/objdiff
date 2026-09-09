# Linked i386 GOT fixtures

`fixture.s` builds two small ELF32 shared libraries and an ordinary relocatable
object. Regenerate with GNU binutils (tested with 2.47):

```sh
sh objdiff-core/tests/data/x86/linked-got/regenerate.sh
cargo test -p objdiff-core --test linked_got
```

The script uses `as --32 -mrelax-relocations=no` and `ld -m elf_i386 -shared
-Bsymbolic --build-id=none`. Disabling assembler relocation relaxation keeps the
ET_REL control fixture within objdiff's existing relocation support. No compiler,
NDK, system i386 runtime, or game assets are needed. Tests read the checked-in
binaries; regeneration is not part of CI.

The right fixture changes text and GOT layout and the size of `object`. Selected
functions deliberately change symbol, addend, register, width, or opcode. Other
functions exercise clobbers, copies, loops, conflicting merges, flags, NOP-prefixed
thunks, and conservative fallbacks. Dynamic slots include R_386_RELATIVE,
R_386_GLOB_DAT, an unsupported R_386_32, ambiguous aliases, overlapping objects,
and a numeric value with no relocation. Tests also mutate metadata and code in
memory to check malformed inputs, missing names, symbol-order independence, and
identical instruction bytes with changed symbolic meaning.

The `direct_*` functions exercise LEA with GOT-relative symbol addresses, including
interior addends, register copies, negative/zero displacements, differing opcodes,
widths and targets, clobbers, ambiguous merges, missing symbols and alias fallbacks.
Direct addresses and pointer loads remain distinct even when they name the same
symbol. Tests also verify that recovery defaults on and can be explicitly disabled.
