    .globl _start
    .text
_start:
    call aligned_ok
    subq $8, %rsp
    call misaligned
    addq $8, %rsp
    movq $7, %rbx
    call clobber
    movq $60, %rax
    xorl %edi, %edi
    syscall
aligned_ok: ret
misaligned: ret
clobber:
    movq $99, %rbx
    ret
