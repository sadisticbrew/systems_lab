# Single-stepping sets the trap flag (bit 8 of RFLAGS) in the real flags. The program must not see
# it: `pushfq` stores RFLAGS and `syscall` saves it in %r11. The exit status is
# ((TF pushed) | (TF in %r11) << 1), so it is 0 when the flags look the same as in a normal run.
    .globl _start
    .text
_start:
    pushfq
    popq %rbx
    shrq $8, %rbx
    andl $1, %ebx            # TF as pushed by pushfq
    movl $39, %eax           # getpid; the syscall leaves RFLAGS in %r11
    syscall
    shrq $8, %r11
    andl $1, %r11d           # TF as saved in %r11
    shll $1, %r11d
    orl %r11d, %ebx
    movl %ebx, %edi
    movl $60, %eax
    syscall
