# Register-width writes: what happens to the rest of %rax when an instruction
# writes only part of it.
    .globl _start
    .text
_start:
    mov    $-1, %rax            # all 64 bits of %rax set
w32:
    mov    $0x11223344, %eax    # write the low 32 bits
    mov    $-1, %rax
w16:
    mov    $0x5566, %ax         # write the low 16 bits
w8:
    mov    $0x77, %al           # write bits 0-7
wh:
    mov    $0x88, %ah           # write bits 8-15
    mov    $0x80, %bl           # %bl = 0x80, which is -128 as a signed byte
zext:
    movzbq %bl, %rcx            # zero-extend the byte into %rcx
sext:
    movsbq %bl, %rdx            # sign-extend the byte into %rdx
    mov    $60, %eax            # exit(0)
    xor    %edi, %edi
    syscall
