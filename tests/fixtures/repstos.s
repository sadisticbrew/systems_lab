    .globl _start
    .text
_start:
    subq $256, %rsp
    movq %rsp, %rdi
    movl $0xab, %eax
    movl $200, %ecx
    rep stosb
    movq $60, %rax
    xorl %edi, %edi
    syscall
