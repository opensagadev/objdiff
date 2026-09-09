# GNU as --32; RIGHT shifts text/GOT and changes selected negative cases.
.ifndef RIGHT
.set RIGHT, 0
.endif
.text
.if RIGHT
.space 32, 0x90
.endif
.macro begin name, reg=ebx, thunk=bx
.globl \name
.type \name, @function
\name:
    call thunk_\thunk
.Lpc\@:
    addl $_GLOBAL_OFFSET_TABLE_+(. - .Lpc\@), %\reg
.endm
.macro end name
    ret
.size \name, .-\name
.endm

begin relative
    mov slot@GOTOFF(%ebx), %eax
end relative
begin named
    mov external@GOT(%ebx), %eax
end named
begin interior
    mov inside@GOTOFF(%ebx), %eax
end interior
begin non_ebx, esi, si
    mov slot@GOTOFF(%esi), %eax
end non_ebx
begin accumulator, eax, ax
    mov slot@GOTOFF(%eax), %eax
end accumulator
begin copied
    mov %ebx, %edi
    xor %ebx, %ebx
    mov slot@GOTOFF(%edi), %eax
end copied
begin loop_equal
    test %eax, %eax
1:  mov slot@GOTOFF(%ebx), %eax
    dec %ecx
    jne 1b
end loop_equal
begin overwritten
    xor %ebx, %ebx
    mov slot@GOTOFF(%ebx), %eax
end overwritten
begin partial_write
    mov $0, %bl
    mov slot@GOTOFF(%ebx), %eax
end partial_write
begin conflicting
    test %eax, %eax
    je 1f
    xor %ebx, %ebx
1:  mov slot@GOTOFF(%ebx), %eax
end conflicting
begin indexed
    mov slot@GOTOFF(%ebx,%ecx,4), %eax
end indexed
begin tls_segment
    mov %fs:slot@GOTOFF(%ebx), %eax
end tls_segment
begin caller_clobber, ecx, cx
    call ordinary
    mov slot@GOTOFF(%ecx), %eax
end caller_clobber
begin callee_saved
    call ordinary
    mov slot@GOTOFF(%ebx), %eax
end callee_saved
begin different_symbol
.if RIGHT
    mov other_slot@GOTOFF(%ebx), %eax
.else
    mov slot@GOTOFF(%ebx), %eax
.endif
end different_symbol
begin different_addend
.if RIGHT
    mov inside@GOTOFF(%ebx), %eax
.else
    mov slot@GOTOFF(%ebx), %eax
.endif
end different_addend
begin different_register
.if RIGHT
    mov slot@GOTOFF(%ebx), %edx
.else
    mov slot@GOTOFF(%ebx), %eax
.endif
end different_register
begin different_width
.if RIGHT
    movw slot@GOTOFF(%ebx), %ax
.else
    mov slot@GOTOFF(%ebx), %eax
.endif
end different_width
begin different_opcode
.if RIGHT
    lea slot@GOTOFF(%ebx), %eax
.else
    mov slot@GOTOFF(%ebx), %eax
.endif
end different_opcode
begin ambiguous
    mov alias_slot@GOTOFF(%ebx), %eax
end ambiguous
begin overlapping
    mov overlap_slot@GOTOFF(%ebx), %eax
end overlapping
begin missing
    mov missing_slot@GOTOFF(%ebx), %eax
end missing
begin unsupported
    mov unsupported_slot@GOTOFF(%ebx), %eax
end unsupported
begin flag_user
    jc 1f
    mov slot@GOTOFF(%ebx), %eax
1:
end flag_user
begin flag_through_thunk
    call thunk_si
    jc 1f
    mov slot@GOTOFF(%ebx), %eax
1:
end flag_through_thunk
begin indirect_jump
    jmp *%eax
    mov slot@GOTOFF(%ebx), %eax
end indirect_jump
.globl numeric
.type numeric,@function
numeric:
    mov $_GLOBAL_OFFSET_TABLE_, %ebx
    mov slot@GOTOFF(%ebx), %eax
end numeric
.globl unproven
.type unproven,@function
unproven:
    call fake_thunk
.Lfake:
    addl $_GLOBAL_OFFSET_TABLE_+(. - .Lfake), %ebx
    mov slot@GOTOFF(%ebx), %eax
end unproven

# Linked R_386_GOTOFF-style direct addresses: no GOT-slot relocation is involved.
begin direct_address
    lea object@GOTOFF(%ebx), %edx
end direct_address
begin direct_interior
    lea object@GOTOFF+2(%ebx), %edx
end direct_interior
begin direct_non_ebx, esi, si
    lea object@GOTOFF(%esi), %edx
end direct_non_ebx
begin direct_negative
    lea ordinary@GOTOFF(%ebx), %edx
end direct_negative
begin direct_zero
    lea (%ebx), %edx
end direct_zero
begin direct_copied
    mov %ebx, %edi
    xor %ebx, %ebx
    lea object@GOTOFF(%edi), %edx
end direct_copied
begin direct_different_symbol
.if RIGHT
    lea other@GOTOFF(%ebx), %edx
.else
    lea object@GOTOFF(%ebx), %edx
.endif
end direct_different_symbol
begin direct_different_addend
    lea object@GOTOFF+RIGHT(%ebx), %edx
end direct_different_addend
begin direct_different_register
.if RIGHT
    lea object@GOTOFF(%ebx), %eax
.else
    lea object@GOTOFF(%ebx), %edx
.endif
end direct_different_register
begin direct_different_opcode
.if RIGHT
    mov slot@GOTOFF(%ebx), %edx
.else
    lea object@GOTOFF(%ebx), %edx
.endif
end direct_different_opcode
begin direct_different_width
.if RIGHT
    lea object@GOTOFF(%ebx), %dx
.else
    lea object@GOTOFF(%ebx), %edx
.endif
end direct_different_width
begin direct_clobber
    xor %ebx, %ebx
    lea object@GOTOFF(%ebx), %edx
end direct_clobber
begin direct_indexed
    lea object@GOTOFF(%ebx,%eax,4), %edx
end direct_indexed
begin direct_conflicting
    test %eax, %eax
    je 1f
    xor %ebx, %ebx
1:  lea object@GOTOFF(%ebx), %edx
end direct_conflicting
begin direct_alias
    lea alias_a@GOTOFF(%ebx), %edx
end direct_alias
begin direct_overlap
    lea overlap@GOTOFF+2(%ebx), %edx
end direct_overlap
begin direct_missing
    lea .Lunnamed@GOTOFF(%ebx), %edx
end direct_missing
begin direct_kills_base
    lea object@GOTOFF(%ebx), %ebx
    lea object@GOTOFF(%ebx), %edx
end direct_kills_base

.macro thunk suffix, reg
.type thunk_\suffix,@function
thunk_\suffix:
    .fill 8,1,0x90
    mov (%esp), %\reg
    ret
.size thunk_\suffix, .-thunk_\suffix
.endm
thunk bx, ebx
thunk si, esi
thunk cx, ecx
thunk ax, eax
.type fake_thunk,@function
fake_thunk:
    xor %ebx, %ebx
    ret
.size fake_thunk, .-fake_thunk
.type ordinary,@function
ordinary:
    ret
.size ordinary, .-ordinary

.section .got,"aw"
slot: .long object
inside: .long object+2
other_slot: .long other
alias_slot: .long alias_a
overlap_slot: .long overlap+2
missing_slot: .long 0x12345678
unsupported_slot: .long external
.if RIGHT
.space 16
.endif
.section .rodata
.Lunnamed: .long 0
.data
.if RIGHT
.space 16
.endif
.globl object, other
.hidden object, other
.type object,@object
object: .long 0x12345678
.if RIGHT
.space 4
.endif
.size object, .-object
.type other,@object
other: .long 0x12345678
.size other, .-other
.type alias_a,@object
.type alias_b,@object
alias_a:
alias_b: .long 0
.size alias_a,4
.size alias_b,4
.type overlap,@object
.type overlap_inner,@object
overlap: .byte 0
overlap_inner: .long 0
.size overlap,5
.size overlap_inner,4
.section .note.GNU-stack,"",@progbits
