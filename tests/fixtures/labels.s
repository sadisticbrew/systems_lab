    .globl _start
    .text
_start:
    mov    $3, %ecx
again:
    call   helper
    dec    %ecx
    jnz    again
    mov    $60, %eax
    xor    %edi, %edi
    syscall
helper:
    ret
