    .globl _start
    .text
_start:
    subq $400, %rsp
    movq $63, %rax
    movq %rsp, %rdi
    syscall
    movq $60, %rax
    xorl %edi, %edi
    syscall
