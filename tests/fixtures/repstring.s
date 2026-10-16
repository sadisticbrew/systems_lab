# rep stos/movs forwards and backwards (DF set), including stores that do not change memory.
    .globl _start
    .text
_start:
    subq $1024, %rsp
    movq %rsp, %r8
    # zero over fresh (zero) stack, forwards: 128 iterations that change nothing
    movq %rsp, %rdi
    xorl %eax, %eax
    movl $128, %ecx
    cld
    rep stosq
    # backwards stosb: 0xab to base+200 down to base+101
    leaq 200(%r8), %rdi
    movl $0xab, %eax
    movl $100, %ecx
    std
    rep stosb
    # backwards movsq: ten quadwords from base+800 down to base+500 (source half zero)
    movabs $0x1122334455667788, %rax
    movq %rax, 800(%r8)
    leaq 800(%r8), %rsi
    leaq 500(%r8), %rdi
    movl $10, %ecx
    rep movsq
    # one non-rep stosl backwards
    leaq 900(%r8), %rdi
    movl $0xdeadbeef, %eax
    stosl
    cld
    # forwards movsb of 16 bytes
    leaq 800(%r8), %rsi
    leaq 600(%r8), %rdi
    movl $16, %ecx
    rep movsb
    movq $60, %rax
    xorl %edi, %edi
    syscall
