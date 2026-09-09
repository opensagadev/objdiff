# Linked ELF32 i386 GOT comparison

The `x86.recoverLinkedGot` option compares proven PIC GOT references symbolically
and defaults to **true**. Set it to **false** to retain numeric comparison. It changes semantic instruction scores, not raw
bytes or data-symbol scores. Other comparison settings retain their existing defaults.

For example, using the CLI built in this checkout:

```sh
cargo build --release -p objdiff-cli
./target/release/objdiff-cli diff \
  -1 /tmp/objdiff-got-validation/original.so \
  -2 /tmp/objdiff-got-validation/rebuilt.so \
  _Z19GizMiniCut_ActivateP7GIZMO_si \
  -c x86.recoverLinkedGot=true -o /tmp/objdiff-got-validation/diff.json
```

The same property appears in the GUI's x86 configuration group and can be supplied
through project options. Reload/rebuild the comparison after changing it: recovery
runs when objects are parsed. Library callers must pass the option both to parsing
and to diff/display. Setting it false leaves the existing numeric comparison and
output unchanged; ET_REL objects, ELF64, and other architectures are unaffected.

## Representation and comparison

`InstructionArg::Recovered` contains an explicit `RecoveredReference`: either a
GOT-slot symbol, a GOT-relative direct symbol address, or a proven GOT-base
construction. Symbol references retain their section and symbol-relative addend. This is not a fabricated ELF relocation. GOT-slot references denote
**loading a pointer from the slot**, not loading the target object's contents. A
subsequent instruction such as `mov eax, [eax]` is left alone. LEA direct addresses
use a separate `GotRelative` target, so they cannot compare as GOT-slot loads.

Recovered operands participate in the normal instruction argument comparison,
diff rows, and scoring. Even identical instruction bytes are compared symbolically
when recovery found a reference: identical displacements can address different
symbols in different libraries. Register operands, opcode, prefixes, and other
arguments still participate. Recovery supports 32-bit pointer loads and 32-bit
LEA direct addresses. Changing their width or replacing MOV with LEA still produces
a mismatch; semantic reference recovery does not erase opcode differences.

Symbol-name normalization uses the existing object-loader normalization function.
As with ordinary instruction relocations, section names must agree when both
symbols have sections; undefined symbols can match defined symbols by name and
addend. Symbol sizes are never an equality condition. Size/content differences
remain visible in independent data comparison.

Deliberately, recovered references require name/addend identity even under relaxed
`functionRelocDiffs` settings. There is no address-only, compiler-generated-name,
or data-value shortcut: equal linked addresses or equal values do not establish
GOT-reference identity. R_386_RELATIVE and R_386_GLOB_DAT both describe a slot
holding the resolved symbol pointer, so their recovered slot references can match.
This comparison does not model runtime symbol interposition.

Display uses `GOT(symbol+addend)`, `GOTOFF(symbol+addend)`, and `GOT_BASE`
in all four x86 syntaxes. These are
annotations, not reassemblable operands. Hover/context menus retain the original
numeric operand and instruction bytes. JSON/protobuf output retains the formatted
opaque argument for compatibility and adds explicit `recovered` metadata, including
`raw_value`; direct-address metadata sets `got_relative=true`. Annotated
instructions also include `raw_bytes`. The ELF `relocation`
field remains absent unless an actual instruction relocation exists.

## Analysis and conservative boundaries

Recovery currently accepts little-endian i386 ET_DYN ELF files with section tables.
GOT slots must be aligned, readable four-byte words in `.got` or `.got.plt` with a
supported dynamic relocation. R_386_GLOB_DAT uses its dynamic symbol (zero addend).
R_386_RELATIVE reads the link-time pointer for REL, or the explicit addend for RELA,
and resolves it through an address index of available static/dynamic symbols.
R_386_32, JMP_SLOT, TLS relocations, and other kinds stay numeric. For LEA,
`base + displacement` resolves directly through the same symbol interval index;
no dynamic relocation is required at that address. This recovers linked GOTOFF
semantics without inventing an instruction relocation.

The base comes from `_GLOBAL_OFFSET_TABLE_` and/or DT_PLTGOT. Conflicting values
reject recovery. If neither is available, the start of `.got.plt` is the supported
ABI fallback; the start of `.got` is not guessed as the base.

Each nonoverlapping, explicitly sized function is decoded once. A direct near call
must point to a semantically validated thunk: up to 16 one-byte NOPs, followed by
`mov r32, [esp]; ret`. The spelling of the thunk symbol is immaterial. The next
instruction must add a 32-bit immediate to that register, producing the metadata
base from the **call's return address**. The accumulator encoding is supported.
The add operand is normalized only if its resulting arithmetic flags are not read
before being overwritten or passing an ABI call/exit boundary.

A worklist propagates unknown/PC/GOT facts through direct control flow. At merges,
a register remains known only when incoming facts agree, including loop backedges.
Instruction-info register writes invalidate facts, including partial and conditional
writes. Full-width register MOV copies are supported. Calls invalidate EAX/ECX/EDX
under the SysV i386 ABI; callee-saved GOT facts survive. No interprocedural analysis
of arbitrary callees is attempted.

Pointer loads and direct LEA addresses require a proven base and no index or
explicit segment override.
Unknown bases, unrelated numeric constants, overwritten registers, ambiguous merges,
indexed/TLS operands, other arithmetic or access forms, malformed instructions,
interior branch targets, and unsupported relocations stay numeric. Entire functions
with indirect jumps or overlapping function extents are excluded. Inferred/zero-size
function ranges, call/pop PIC, spills/reloads of the base, and other thunk encodings
are outside this initial implementation.

## Alias and interior-address policy

Identical static/dynamic symbol records are deduplicated. For relative relocations,
an exact address or an interior address must belong to exactly one named symbol.
Distinct aliases, overlapping ranges, and same-name records with conflicting sizes
are ambiguous, even at an exact start. Zero-size symbols can resolve only their
exact address. There is no nearest-symbol heuristic, and symbol iteration order
cannot affect the choice. Direct LEA targets follow the same conservative alias/interior policy as relative
relocations. Named GLOB_DAT relocations already identify a symbol and
do not need address-based alias selection. Missing names and out-of-range metadata
fall back to numeric operands.

The resolver builds disjoint address intervals and indexes slots once per object;
there is no symbol-table scan per instruction. Thunk validation and recovered
instruction references are cached. Recovery does eagerly analyze all eligible
functions during loading, even for a single-function diff.

## Initial GOT-slot implementation validation, 2026-09-09

Ten synthetic integration tests cover positive references, differing symbol sizes,
real changes, all formatter styles, exported metadata/raw bytes, disabled recovery,
ET_REL compatibility, malformed metadata/code, aliases/overlaps, missing GOT symbols,
order independence, and byte-identical instructions with different targets. Sources
and regeneration instructions are in `tests/data/x86/linked-got`.

Checks passed: `cargo test` (66 tests), `cargo +nightly fmt --all`,
`cargo +nightly clippy --all-targets --all-features --workspace -- -D warnings`,
`cargo check -p objdiff-core --no-default-features --features x86`, `cargo build`,
and `cargo build --release`.

`cargo deny check` passed bans/licenses/sources but failed existing advisories for
h2 (RUSTSEC-2026-0258) and webbrowser (RUSTSEC-2026-0257). Cargo.lock and dependency
versions were unchanged. This feature enables iced-x86's existing `instr_info`
feature. The advisory database and temporary cargo-deny installation were kept
outside the repository.

Saga libraries were copied before comparison; neither the Saga workspace nor its
installed objdiff binary was modified. Snapshot SHA-256:

- Original: `d864055b1db5cc2ee2c16f7968ed68965b69f262ace6b6bfe43558296981c967`
- Rebuilt: `22fded4687410105f983249b2765faeee0576567ee73879842c172f8389adbfb`

Independent readelf/objdump inspection and file-offset reads established:

| | Original | Rebuilt |
|---|---|---|
| GOT base | `0x616870` | `0x3ead30` |
| Pointer-load instruction | `0x4d93f7` | `0x1b83a7` |
| EBX displacement | `-0x47bc` | `-0x3484` |
| GOT slot | `0x6120b4` | `0x3e78ac` |
| Dynamic relocation | R_386_RELATIVE | R_386_RELATIVE |
| Stored link-time pointer | `0x680a20` | `0x4701c4` |
| Target | `editor_active` | `editor_active` |
| GOT setup add immediate | `0x13d607` | `0x232b17` |

With `/home/fabian/git/objdiff/target/release/objdiff-cli`, both selected operands
change from argument mismatches to semantic matches: `add ebx, GOT_BASE` and
`mov eax, [ebx+GOT(editor_active)]`. The function still has 16 mismatched rows
(previously 18). For example, `mov esi, [esp+0x94]` versus `mov eax, [esp+0x94]`,
the subsequent TEST registers, and JNE versus JE/control-flow differences remain.
The score moves from 92.41739 to 92.434784; the two explained matches, rather than
a desired percentage, are the acceptance evidence.

Additional checks:

| Function | Recovered slot targets | Mismatched rows, disabled → enabled |
|---|---|---|
| `NuSoundAppTerminate` | `NuSound` | 2 → 0 |
| `NuIOS_RecordFlurryEvent` | `g_javaVM`, `g_activityClass` | 5 → 2 |
| `GizMiniCut_Reset` | Base construction only | 15 → 14 |
| `GizMiniCut_Load` | Base construction only | 1 → 0 |
| `GizMiniCut_ReserveBufferSpace` | Base construction only | 58 → 57 |

Runtime measurements use the same snapshots and release CLI, include loading both
objects and writing the selected-function JSON, and alternate disabled/enabled
runs after warmup. Seven measured runs per mode are recorded in the temporary
validation artifacts. The median was **128 ms disabled** (126–138 ms range) and
**401 ms enabled** (386–632 ms range): approximately **274 ms added**, or 3.14×
end-to-end time. This is a single-function invocation that pays for eager recovery
across both libraries; cached display/diff does not rerun the analysis. These
semantic matches do not imply byte-identical code.

## Direct-address extension and default-on validation

Recovery now also handles `lea r32, [proven_got_base + displacement]` and is enabled
by default. Existing helpers that omit the option receive recovery when using the
updated binary. Explicit `x86.recoverLinkedGot=false` still opts out; saved explicit
false values are respected. The eager object-load analysis cost described above
therefore applies by default to eligible linked i386 libraries.

Fifteen integration tests now cover both reference kinds, including direct-address
addends, differing sizes, negative/zero displacements, all formatter styles,
register/opcode/width changes, register invalidation, aliases, indexed addressing,
ambiguous merges, identical LEA bytes with changed symbol identity, export metadata,
and the default/opt-out behavior. `cargo test` passes 71 tests; formatting, full
workspace clippy, no_std x86 checking, and debug/release builds pass. There are no
new dependencies in this extension.

Fresh temporary Saga snapshots were used; no game files are included as fixtures.
The original SHA-256 is unchanged from the initial validation; the new rebuilt
snapshot is `7767b93ce940274846b3a37d82d4442f063abe76db51b5eb18a61403257a9cb9`.
Readelf and objdump independently identify these addresses for `NuPadGetPlayer(int)`
(`_Z14NuPadGetPlayeri`):

| | Original | Rebuilt |
|---|---|---|
| GOT base | `0x616870` | `0x3f3d20` |
| First LEA | `0x26bcb1` | `0x2bb7b0` |
| Second LEA | `0x26bcce` | `0x2bb7cd` |
| Displacement | `0x137670` | `0xc32940` |
| Direct target address | `0x74dee0` | `0x1026660` |
| Symbol | `_ZL18g_nupadScannedPads` | `_ZL18g_nupadScannedPads` |
| Addend | 0 | 0 |

The workspace release CLI reports **100%** with no explicit option: the GOT setup
and both LEAs match symbolically. Explicitly disabling recovery reports
**99.90323%**, with JSON output identical to the previous commit's disabled output.
The previous GOT-slot-only implementation reported **99.935486%** when enabled.
Both LEAs display `lea edx, [ebx+GOTOFF(_ZL18g_nupadScannedPads)]`; exported metadata
distinguishes the direct address and retains the different raw displacement/bytes.
These are semantic matches, not byte identity.

`NuPadGetPort(int)` and `NuPadMapPortToPS2Port(int, int)` also reach semantic 100%.
`NuPadSetMotors` retains a numeric mismatch for a direct MOV from object contents
outside the GOT. Direct memory loads/stores outside GOT slots remain unsupported;
this extension recovers LEA address calculations only.
