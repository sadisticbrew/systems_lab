# The legacy 32-bit system call entry: uname(2) through `int $0x80` writes the buffer in the kernel.
# Only 32 bits of each argument reach the kernel, so the buffer lives in .bss, below 4 GiB.
    .globl _start
    .bss
buf: .skip 400
    .text
_start:
    movl $122, %eax          # 32-bit ABI uname
    movl $buf, %ebx
    int $0x80
    movl $1, %eax            # 32-bit ABI exit
    xorl %ebx, %ebx
    int $0x80
